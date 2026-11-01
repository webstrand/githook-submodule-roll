//! The versions that tags name, and which of them a submodule may roll forward to.

use std::cmp::Ordering;

/// A version that a tag such as `v1.2.3`, `1.9` or `v2.0.0-rc.1` names. A missing minor or patch
/// number is 0, and build metadata after `+` is ignored, as semver orders versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    numbers: [u64; 3],
    pre: Option<String>,
}

impl Version {
    /// The version that `tag` names, or `None` when it names none.
    pub fn parse(tag: &str) -> Option<Version> {
        let version = tag.strip_prefix('v').unwrap_or(tag);
        let version = version
            .split_once('+')
            .map_or(version, |(version, _)| version);
        let (core, pre) = match version.split_once('-') {
            Some((_, "")) => return None,
            Some((core, pre)) => (core, Some(pre.to_owned())),
            None => (version, None),
        };
        let mut numbers = [0; 3];
        let mut parts = core.split('.');
        for (i, part) in parts.by_ref().take(3).enumerate() {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            numbers[i] = part.parse().ok()?;
        }
        if parts.next().is_some() {
            return None;
        }
        Some(Version { numbers, pre })
    }

    pub fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }

    /// Whether `other` makes no breaking change from this version, as cargo reads semver: a
    /// version from 1.0.0 on is compatible with those of the same major number, and one before
    /// 1.0.0 with those of the same minor number.
    pub fn is_compatible(&self, other: &Version) -> bool {
        let [major, minor, _] = self.numbers;
        if major != 0 {
            other.numbers[0] == major
        } else {
            other.numbers[0] == 0 && other.numbers[1] == minor
        }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        self.numbers
            .cmp(&other.numbers)
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => compare_pre(a, b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Orders pre-release labels as semver does: identifier by identifier, numbers numerically and
/// below words, and a label below the longer labels that it begins.
fn compare_pre(a: &str, b: &str) -> Ordering {
    let number = |id: &str| {
        id.parse::<u64>()
            .ok()
            .filter(|_| id.bytes().all(|b| b.is_ascii_digit()))
    };
    let mut a = a.split('.');
    let mut b = b.split('.');
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let order = match (number(x), number(y)) {
                    (Some(x), Some(y)) => x.cmp(&y),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => x.cmp(y),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    fn v(tag: &str) -> Version {
        Version::parse(tag).unwrap()
    }

    #[test]
    fn parses_tags_with_and_without_prefix_and_parts() {
        assert_eq!(v("v1.9"), v("1.9.0"));
        assert_eq!(v("v2"), v("2.0.0"));
        assert_eq!(v("v1.2.3+build.5"), v("1.2.3"));
        assert!(v("v1.0.0-rc.1").is_prerelease());
        for tag in [
            "release", "v", "v1.", "v1.2.3.4", "v1.x", "va1", "v1.2-", "v-1",
        ] {
            assert_eq!(Version::parse(tag), None, "{tag}");
        }
    }

    #[test]
    fn orders_as_semver() {
        let ordered = [
            "0.9.9",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.2.3",
            "1.10.0",
            "2.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn compatible_within_a_major_or_a_zero_minor() {
        assert!(v("1.2.3").is_compatible(&v("1.9.0")));
        assert!(!v("1.2.3").is_compatible(&v("2.0.0")));
        assert!(v("0.3.1").is_compatible(&v("0.3.9")));
        assert!(!v("0.3.1").is_compatible(&v("0.4.0")));
        assert!(!v("0.3.1").is_compatible(&v("1.3.1")));
    }
}
