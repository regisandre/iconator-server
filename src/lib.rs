//! File and folder icon lookup for IDE project trees.
//!
//! The associations come from vscode-icons. `build.rs` lowercases their keys and compiles them
//! into three fst maps embedded in the binary; lookups follow VS Code's matching rules.

use std::{path::Path, sync::LazyLock};

use fst::Map;

/// Opens, on first use, a map that `build.rs` wrote to `OUT_DIR` and that is embedded in the binary.
macro_rules! embedded_map {
    ($file:literal) => {
        LazyLock::new(|| {
            Map::new(include_bytes!(concat!(env!("OUT_DIR"), "/", $file)).as_slice())
                .expect("build.rs writes valid fst maps")
        })
    };
}

static FILENAME_ICONS: LazyLock<Map<&[u8]>> = embedded_map!("filename_icons.bin");
static EXT_ICONS: LazyLock<Map<&[u8]>> = embedded_map!("ext_icons.bin");
static FOLDER_ICONS: LazyLock<Map<&[u8]>> = embedded_map!("folder_icons.bin");

/// Returns the icon ID for a file.
///
/// Matching is case-insensitive. The full file name is tried first, then every dotted suffix
/// from the longest: `index.d.ts` tries `index.d.ts`, `d.ts`, then `ts`, and `.env` tries `env`.
///
/// Returns `None` when there is no match or when the path has no file name (e.g. `""`, `"/"`, `".."`).
pub fn get_icon_for_file(path: impl AsRef<Path>) -> Option<u64> {
    let name = lowercase_name(path.as_ref())?;

    FILENAME_ICONS.get(&name).or_else(|| {
        name.iter()
            .enumerate()
            .filter(|&(_, &byte)| byte == b'.')
            .find_map(|(dot, _)| EXT_ICONS.get(&name[dot + 1..]))
    })
}

/// Returns the icon ID for a folder, matching its name case-insensitively.
///
/// Returns `None` when there is no match or when the path has no file name (e.g. `""`, `"/"`, `".."`).
pub fn get_icon_for_folder(path: impl AsRef<Path>) -> Option<u64> {
    FOLDER_ICONS.get(lowercase_name(path.as_ref())?)
}

/// The last path component in ASCII lowercase. Every key in the tables is ASCII, so this
/// matches VS Code's lowercasing for any name that can match at all.
fn lowercase_name(path: &Path) -> Option<Vec<u8>> {
    Some(path.file_name()?.as_encoded_bytes().to_ascii_lowercase())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn common_languages() {
        assert_eq!(get_icon_for_file("./test.js"), Some(296));
        assert_eq!(get_icon_for_file("./test.ts"), Some(633));
        assert_eq!(get_icon_for_file("./test.jsx"), Some(508));
        assert_eq!(get_icon_for_file("./test.tsx"), Some(529));
        assert_eq!(get_icon_for_file("./test.json"), Some(284));
        assert_eq!(get_icon_for_file("./test.yaml"), Some(708));
        assert_eq!(get_icon_for_file("./test.yml"), Some(708));
        assert_eq!(get_icon_for_file("./test.md"), Some(361));
        assert_eq!(get_icon_for_file("./test.css"), Some(116));
        assert_eq!(get_icon_for_file("./test.html"), Some(255));
        assert_eq!(get_icon_for_file("./test.rs"), Some(525));
    }

    #[test]
    fn common_folders() {
        assert_eq!(get_icon_for_folder("./.github"), Some(862));
        assert_eq!(get_icon_for_folder("./src"), Some(1054));
        assert_eq!(get_icon_for_folder("./tests"), Some(1084));
    }

    /// The tables come from vscode-icons, so names match the way VS Code matches them:
    /// case-insensitive, and the longest dotted suffix wins (`d.ts` before `ts`).
    #[test]
    fn names_match_like_vscode() {
        for (path, expected, case) in [
            ("Cargo.toml", 85, "file name in another case"),
            ("pulumi.yaml", 425, "file name stored with capitals"),
            ("README.MD", 361, "extension in another case"),
            ("src/index.d.ts", 618, "multi-part extension"),
            ("app.spec.ts", 619, "multi-part extension"),
            (".env", 154, "dotfile tail used as extension"),
        ] {
            assert_eq!(get_icon_for_file(path), Some(expected), "{case}: {path}");
        }
        assert_eq!(
            get_icon_for_folder("./.GitHub"),
            Some(862),
            "folder in another case"
        );
    }

    #[test]
    fn paths_without_file_name_return_none() {
        for path in ["", "/", ".", ".."] {
            assert_eq!(get_icon_for_file(path), None, "file {path:?}");
            assert_eq!(get_icon_for_folder(path), None, "folder {path:?}");
        }
    }

    #[test]
    fn unknown_file_without_extension_returns_none() {
        assert_eq!(get_icon_for_file("foo"), None);
        assert_eq!(get_icon_for_file("../../etc/passwd"), None);
    }
}
