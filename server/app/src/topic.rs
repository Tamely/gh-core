//! WebSocket topics: `room:{id}`, `team:{id}`, `mentor-tag:{tag}`, `user:{id}`.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TopicKind {
    /// Judging queue of one presentation room.
    Room,
    /// Judge Room Radar and heartbeat for one team.
    Team,
    /// Mentor ticket pins for one skill tag.
    MentorTag,
    /// Personal alerts for one user.
    User,
}

impl TopicKind {
    pub const ALL: [TopicKind; 4] = [Self::Room, Self::Team, Self::MentorTag, Self::User];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Room => "room",
            Self::Team => "team",
            Self::MentorTag => "mentor-tag",
            Self::User => "user",
        }
    }
}

/// A validated topic name. Holding one means the string is well-formed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Topic {
    name: String,
    kind: TopicKind,
}

pub const MAX_ID_LEN: usize = 64;

impl Topic {
    pub fn parse(name: &str) -> Result<Self, &'static str> {
        let (prefix, id) = name.split_once(':').ok_or("topic must look like kind:id")?;
        let kind = TopicKind::ALL
            .into_iter()
            .find(|k| k.as_str() == prefix)
            .ok_or("unknown topic kind")?;
        if id.is_empty() || id.len() > MAX_ID_LEN {
            return Err("topic id must be 1 to 64 characters");
        }
        // Tags like "C++" and "C#" are allowed; ':' and spaces are not.
        let allowed = |c: char| c.is_ascii_alphanumeric() || "-_.+#".contains(c);
        if !id.chars().all(allowed) {
            return Err("topic id has characters that aren't allowed");
        }
        Ok(Self {
            name: name.to_owned(),
            kind,
        })
    }

    pub fn kind(&self) -> TopicKind {
        self.kind
    }

    pub fn id(&self) -> &str {
        &self.name[self.kind.as_str().len() + 1..]
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_kind() {
        for (name, kind, id) in [
            ("room:PFT-1200", TopicKind::Room, "PFT-1200"),
            (
                "team:5b1c9e2a-0000-4000-8000-000000000000",
                TopicKind::Team,
                "5b1c9e2a-0000-4000-8000-000000000000",
            ),
            ("mentor-tag:C++", TopicKind::MentorTag, "C++"),
            ("mentor-tag:C#", TopicKind::MentorTag, "C#"),
            ("user:42", TopicKind::User, "42"),
        ] {
            let topic = Topic::parse(name).unwrap();
            assert_eq!(topic.kind(), kind);
            assert_eq!(topic.id(), id);
            assert_eq!(topic.as_str(), name);
        }
    }

    #[test]
    fn rejects_malformed_names() {
        for name in [
            "",
            "room",
            "room:",
            ":42",
            "rooms:1",
            "ROOM:1",
            "room:has space",
            "room:a:b",
            "room:*",
            "user:é",
            &format!("user:{}", "x".repeat(MAX_ID_LEN + 1)),
        ] {
            assert!(Topic::parse(name).is_err(), "{name:?} should be rejected");
        }
    }

    #[test]
    fn accepts_longest_id() {
        assert!(Topic::parse(&format!("user:{}", "x".repeat(MAX_ID_LEN))).is_ok());
    }
}
