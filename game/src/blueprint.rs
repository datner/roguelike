use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::cell::Rgb;

#[derive(Debug, Clone, Deserialize)]
pub struct EntityBlueprint {
    pub name: String,
    pub glyph: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub blocks_movement: bool,
}

#[derive(Debug)]
pub struct BlueprintError(BlueprintErrorKind);

/// `ron::from_str` reports parse failures as `SpannedError` (with line/col),
/// while I/O failures convert into a plain `ron::error::Error`; keep both.
#[derive(Debug)]
enum BlueprintErrorKind {
    Ron(ron::error::Error),
    Parse(ron::error::SpannedError),
}

impl std::fmt::Display for BlueprintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            BlueprintErrorKind::Ron(err) => err.fmt(f),
            BlueprintErrorKind::Parse(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for BlueprintError {}

impl From<ron::error::Error> for BlueprintError {
    fn from(err: ron::error::Error) -> Self {
        Self(BlueprintErrorKind::Ron(err))
    }
}

impl From<ron::error::SpannedError> for BlueprintError {
    fn from(err: ron::error::SpannedError) -> Self {
        Self(BlueprintErrorKind::Parse(err))
    }
}

impl From<std::io::Error> for BlueprintError {
    fn from(err: std::io::Error) -> Self {
        Self(BlueprintErrorKind::Ron(ron::error::Error::from(err)))
    }
}

pub fn load_blueprint(path: &Path) -> Result<EntityBlueprint, BlueprintError> {
    let contents = fs::read_to_string(path)?;
    Ok(ron::from_str(&contents)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_rubble_blueprint_from_the_data_directory() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/entities/rubble.ron");
        let blueprint = load_blueprint(&path).expect("rubble.ron should parse");
        assert_eq!(blueprint.name, "rubble");
        assert_eq!(blueprint.glyph, '%');
        assert_eq!(blueprint.fg, Rgb(180, 140, 90));
        assert!(blueprint.blocks_movement);
    }

    #[test]
    fn missing_file_is_a_blueprint_error() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/entities/does-not-exist.ron");
        assert!(load_blueprint(&path).is_err());
    }
}
