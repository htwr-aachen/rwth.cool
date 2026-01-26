// Fuzzy match function that returns match indices
function fuzzyMatchWithIndices(text, search) {
  text = text.toLowerCase();
  search = search.toLowerCase();

  const indices = [];
  let searchIndex = 0;

  for (let i = 0; i < text.length && searchIndex < search.length; i++) {
    if (text[i] === search[searchIndex]) {
      indices.push(i);
      searchIndex++;
    }
  }

  return searchIndex === search.length ? indices : null;
}

// Highlight matched characters in text
function highlightMatches(text, indices) {
  if (!indices || indices.length === 0) return text;

  let result = "";
  for (let i = 0; i < text.length; i++) {
    if (indices.includes(i)) {
      result += `<mark>${text[i]}</mark>`;
    } else {
      result += text[i];
    }
  }
  return result;
}

// Calculate Levenshtein distance for similarity matching
function levenshteinDistance(a, b) {
  a = a.toLowerCase();
  b = b.toLowerCase();
  
  if (a.length === 0) return b.length;
  if (b.length === 0) return a.length;

  const matrix = [];

  for (let i = 0; i <= b.length; i++) {
    matrix[i] = [i];
  }
  for (let j = 0; j <= a.length; j++) {
    matrix[0][j] = j;
  }

  for (let i = 1; i <= b.length; i++) {
    for (let j = 1; j <= a.length; j++) {
      if (b.charAt(i - 1) === a.charAt(j - 1)) {
        matrix[i][j] = matrix[i - 1][j - 1];
      } else {
        matrix[i][j] = Math.min(
          matrix[i - 1][j - 1] + 1,
          matrix[i][j - 1] + 1,
          matrix[i - 1][j] + 1
        );
      }
    }
  }

  return matrix[b.length][a.length];
}

// Calculate match score (lower is better)
// Returns null if no match found
function calculateScore(key, description, aliases, search) {
  const keyMatch = fuzzyMatchWithIndices(key, search);
  const descMatch = fuzzyMatchWithIndices(description, search);
  
  // Handle aliases as string or array
  let aliasMatch = null;
  if (Array.isArray(aliases)) {
    for (const alias of aliases) {
      aliasMatch = fuzzyMatchWithIndices(alias, search);
      if (aliasMatch) break;
    }
  } else {
    aliasMatch = fuzzyMatchWithIndices(aliases, search);
  }

  // Priority: key > aliases > description
  if (keyMatch) return { score: 1, type: "key", indices: keyMatch };
  if (aliasMatch) return { score: 2, type: "aliases", indices: aliasMatch };
  if (descMatch) return { score: 3, type: "description", indices: descMatch };

  return null;
}

// Calculate match score with Levenshtein fallback for 404 suggestions
function calculateScoreWithFallback(key, description, aliases, search) {
  // Try exact fuzzy match first
  const exactMatch = calculateScore(key, description, aliases, search);
  if (exactMatch) return exactMatch;

  // Fall back to Levenshtein distance for partial matches
  const keyDistance = levenshteinDistance(key, search);
  
  let minAliasDistance = Infinity;
  if (Array.isArray(aliases)) {
    for (const alias of aliases) {
      const dist = levenshteinDistance(alias, search);
      if (dist < minAliasDistance) minAliasDistance = dist;
    }
  } else if (aliases) {
    minAliasDistance = levenshteinDistance(aliases, search);
  }
  
  // Only consider matches with reasonable distance (less than half the search length + 3)
  const threshold = Math.floor(search.length / 2) + 3;
  
  if (keyDistance <= threshold) {
    return { score: 10 + keyDistance, type: "similar", indices: null, distance: keyDistance };
  }
  if (minAliasDistance <= threshold) {
    return { score: 20 + minAliasDistance, type: "similar", indices: null, distance: minAliasDistance };
  }

  return null;
}
