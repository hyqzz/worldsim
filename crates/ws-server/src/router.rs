//! Minimal router helper — pattern `/:param` segments.
//!
//! The API server uses plain string comparisons for its 12 endpoints, but this
//! module provides a reusable tester and documents the path contract.

/// A pattern segment.
#[derive(Debug, Clone)]
pub enum Seg {
    Lit(String),
    Param(String),
}

/// Compiled route pattern.
#[derive(Debug, Clone)]
pub struct Pattern {
    segs: Vec<Seg>,
}

impl Pattern {
    pub fn compile(pat: &str) -> Self {
        let segs = pat
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| {
                if let Some(name) = s.strip_prefix(':') {
                    Seg::Param(name.to_string())
                } else {
                    Seg::Lit(s.to_string())
                }
            })
            .collect();
        Self { segs }
    }

    /// Match `path` against this pattern.  Returns captured params on success.
    pub fn matches(&self, path: &str) -> Option<Vec<(String, String)>> {
        let parts: Vec<&str> = path
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() != self.segs.len() {
            return None;
        }
        let mut out = Vec::new();
        for (seg, part) in self.segs.iter().zip(parts) {
            match seg {
                Seg::Lit(lit) => {
                    if lit != part {
                        return None;
                    }
                }
                Seg::Param(name) => out.push((name.clone(), part.to_string())),
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pattern_basics() {
        let p = Pattern::compile("/v1/worlds/:id/control");
        assert!(p.matches("/v1/worlds/abc/control").is_some());
        assert!(p.matches("/v1/worlds/abc").is_none());
        let cap = p.matches("/v1/worlds/my-id/control").unwrap();
        assert_eq!(cap[0].1, "my-id");
    }
}
