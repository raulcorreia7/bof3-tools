//! A route: the buttons a run presses, frame by frame, stored as a TOML file.
//!
//! ```toml
//! name = "new-game"
//! frames = 8100
//!
//! [[press]]
//! buttons = ["start"]
//! from = 900      # first frame
//! until = 2300    # first frame after the press stops; absent runs to the end
//! every = 90      # the press repeats every 90 frames
//! hold = 4        # and holds the buttons for 4 of them
//! ```

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::Error;

/// The routes the repository carries, by name.
const BUILT_IN: [(&str, &str); 1] = [("new-game", include_str!("../routes/new-game.toml"))];

/// The pad's button bits in the word the kernel returns, pressed as 1 (psx-spx "Controllers and
/// Memory Cards", "Standard Digital/Analog Controllers": the switch bits of the second and third
/// response bytes, inverted).
const BUTTONS: [(&str, u16); 16] = [
    ("select", 1 << 0),
    ("l3", 1 << 1),
    ("r3", 1 << 2),
    ("start", 1 << 3),
    ("up", 1 << 4),
    ("right", 1 << 5),
    ("down", 1 << 6),
    ("left", 1 << 7),
    ("l2", 1 << 8),
    ("r2", 1 << 9),
    ("l1", 1 << 10),
    ("r1", 1 << 11),
    ("triangle", 1 << 12),
    ("circle", 1 << 13),
    ("cross", 1 << 14),
    ("square", 1 << 15),
];

/// A button script: how long the run lasts and what it presses when.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    /// The route's name, which also names its cache file.
    pub name: String,
    /// Frames the run lasts.
    pub frames: u64,
    presses: Vec<Press>,
    /// The SHA-256 of the route's text, so a cache knows when the route changed.
    pub sha: String,
}

/// One repeating press.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Press {
    buttons: u16,
    from: u64,
    until: Option<u64>,
    every: u64,
    hold: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteFile {
    name: String,
    frames: u64,
    #[serde(default)]
    press: Vec<PressFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PressFile {
    buttons: Vec<String>,
    from: u64,
    until: Option<u64>,
    every: u64,
    hold: u64,
}

impl Route {
    /// Reads a route from its TOML text.
    ///
    /// # Errors
    ///
    /// [`Error::Route`] when the text is not a route: bad TOML, an unknown button, a press that
    /// repeats every 0 frames or holds longer than it repeats.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let file: RouteFile = toml::from_str(text).map_err(|error| Error::Route(error.to_string()))?;
        if file.name.is_empty() || !file.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(Error::Route(format!("route name {:?} is not letters, digits, - and _", file.name)));
        }
        let mut presses = Vec::new();
        for press in file.press {
            let mut buttons = 0;
            for name in &press.buttons {
                let bit = BUTTONS.iter().find(|(button, _)| button.eq_ignore_ascii_case(name)).map(|(_, bit)| *bit).ok_or_else(|| Error::Route(format!("unknown button {name:?}")))?;
                buttons |= bit;
            }
            if press.every == 0 || press.hold == 0 || press.hold > press.every {
                return Err(Error::Route(format!("a press from frame {} holds {} of every {} frames; hold 1 to every frames", press.from, press.hold, press.every)));
            }
            presses.push(Press { buttons, from: press.from, until: press.until, every: press.every, hold: press.hold });
        }
        let sha = hex(&Sha256::digest(text.as_bytes()));
        Ok(Self { name: file.name, frames: file.frames, presses, sha })
    }

    /// The route the repository carries under `name`.
    pub fn built_in(name: &str) -> Option<Self> {
        BUILT_IN.iter().find(|(known, _)| *known == name).and_then(|(_, text)| Self::parse(text).ok())
    }

    /// The names of the routes the repository carries.
    pub fn built_in_names() -> impl Iterator<Item = &'static str> {
        BUILT_IN.iter().map(|(name, _)| *name)
    }

    /// The buttons held during `frame`, pressed as 1.
    pub fn buttons(&self, frame: u64) -> u16 {
        self.presses
            .iter()
            .filter(|press| frame >= press.from && press.until.is_none_or(|until| frame < until))
            .filter(|press| (frame - press.from) % press.every < press.hold)
            .fold(0, |buttons, press| buttons | press.buttons)
    }
}

/// Lower-case hex of `bytes`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: u16 = 1 << 3;
    const CROSS: u16 = 1 << 14;

    #[test]
    fn the_new_game_route_presses_start_then_cross() {
        let route = Route::built_in("new-game").expect("the built-in route parses");
        assert_eq!(route.buttons(899), 0);
        assert_eq!(route.buttons(900), START);
        assert_eq!(route.buttons(903), START);
        assert_eq!(route.buttons(904), 0);
        assert_eq!(route.buttons(990), START);
        assert_eq!(route.buttons(2340), 0, "the Start press stops at frame 2300");
        assert_eq!(route.buttons(2400), CROSS);
        assert_eq!(route.buttons(2460), CROSS);
        assert_eq!(route.buttons(2464), 0);
    }

    #[test]
    fn a_bad_route_says_what_is_wrong() {
        let unknown = "name = \"x\"\nframes = 1\n[[press]]\nbuttons = [\"jump\"]\nfrom = 0\nevery = 1\nhold = 1\n";
        assert!(matches!(Route::parse(unknown), Err(Error::Route(message)) if message.contains("jump")));
        let long = "name = \"x\"\nframes = 1\n[[press]]\nbuttons = [\"start\"]\nfrom = 0\nevery = 2\nhold = 3\n";
        assert!(matches!(Route::parse(long), Err(Error::Route(message)) if message.contains("holds 3")));
        assert!(Route::parse("name = \"../x\"\nframes = 1\n").is_err(), "a name cannot leave the cache folder");
    }
}
