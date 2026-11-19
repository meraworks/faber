/// Names the bad name and up to three near names. Without a near name, it gives the hint.
pub fn unknown(what: &str, name: &str, candidates: &[&str], hint: &str) -> String {
    let near = nearest(name, candidates);
    if near.is_empty() {
        format!("Unknown {what} {name}. {hint}")
    } else {
        format!("Unknown {what} {name}. Nearest: {}.", near.join(", "))
    }
}

/// Up to three candidates within a small edit distance, the closest first.
pub fn nearest<'a>(name: &str, candidates: &[&'a str]) -> Vec<&'a str> {
    let limit = (name.chars().count() / 3).max(2);
    let mut scored: Vec<(usize, &str)> = candidates
        .iter()
        .map(|candidate| (distance(name, candidate), *candidate))
        .filter(|(distance, _)| *distance <= limit)
        .collect();
    scored.sort();
    scored.into_iter().take(3).map(|(_, name)| name).collect()
}

/// The Levenshtein distance between two names, counted in characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != *cb))
                .min(above + 1)
                .min(row[j] + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}
/// A display name stored inside the project.
pub fn valid_view(name: &str) -> bool {
    !name.is_empty()
        && name == name.trim()
        && name.chars().count() <= 100
        && !name.chars().any(char::is_control)
}
