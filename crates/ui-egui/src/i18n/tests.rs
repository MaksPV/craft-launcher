//! Tests for the i18n catalogs (format, duplicates, placeholders, plural forms, coverage) and the
//! lookups.

use std::collections::{BTreeSet, HashSet};

use super::catalog::{Catalog, unescape};
use super::lang::plural_east_slavic;
use super::{LANGUAGES, Lang, LangInfo, lang_from_tag, message, tr, trn};

/// Each catalog's rows as (line number, context, source, translation), split exactly as the
/// parser does: `trim_end` only, since a plain row *starts* with a TAB.
fn rows(lang: &LangInfo) -> Vec<(usize, &str, &str, &str)> {
    let mut out = Vec::new();
    for (i, line) in lang.source.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [context, source, translation] = fields.as_slice() else {
            panic!("{} line {}: expected 3 tab-separated fields: {line:?}", lang.code, i + 1);
        };
        out.push((i + 1, *context, *source, *translation));
    }
    out
}

fn catalogs() -> impl Iterator<Item = &'static LangInfo> {
    LANGUAGES.iter().filter(|l| !l.source.is_empty())
}

fn placeholders(template: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(len) = rest[open..].find('}') else { break };
        let name = &rest[open + 1..open + len];
        if !name.is_empty() {
            out.insert(name.to_string());
        }
        rest = &rest[open + len + 1..];
    }
    out
}

#[test]
fn catalogs_have_rows_and_no_empty_translations() {
    for lang in catalogs() {
        let rows = rows(lang);
        assert!(rows.iter().filter(|r| r.1.is_empty()).count() > 50, "{}: plain rows were skipped", lang.code);
        for (line, _, source, translation) in rows {
            assert!(!source.is_empty() && !translation.is_empty(), "{} line {line}: empty field", lang.code);
        }
    }
}

#[test]
fn no_duplicate_keys() {
    for lang in catalogs() {
        let mut seen = HashSet::new();
        for (line, context, source, _) in rows(lang) {
            assert!(seen.insert((context, source)), "{} line {line}: duplicate key {context:?} {source:?}", lang.code);
        }
    }
}

#[test]
fn placeholders_match_in_translation() {
    for lang in catalogs() {
        for (line, context, source, translation) in rows(lang) {
            if context == "@id" {
                continue; // keyed by id: the English label isn't in the row
            }
            // Translations may reorder placeholders, but must keep each one.
            let (src, tgt) = (placeholders(source), placeholders(translation));
            assert_eq!(src, tgt, "{} line {line}: placeholders differ: {source:?} -> {translation:?}", lang.code);
        }
    }
}

#[test]
fn plural_forms_count_matches_language() {
    for lang in catalogs() {
        for (line, context, _, translation) in rows(lang) {
            if context == "@plural" {
                let forms = translation.split('|').count();
                assert_eq!(forms, lang.plural_forms, "{} line {line}: {translation:?}", lang.code);
            }
        }
        // The plural rule never asks for a form the catalog doesn't have.
        for n in 0..200 {
            assert!((lang.plural)(n) < lang.plural_forms, "{}: {n}", lang.code);
        }
    }
}

#[test]
fn every_msg_row_translates_its_own_text() {
    for lang in catalogs() {
        let cat = Catalog::parse(lang.source);
        for (line, context, source, translation) in rows(lang) {
            if context == "@msg" {
                assert_eq!(cat.message(&unescape(source)).as_deref(), Some(unescape(translation).as_str()), "{} line {line}", lang.code);
            }
        }
    }
}

#[test]
fn ellipsis_preserved() {
    for lang in catalogs() {
        for (line, context, source, translation) in rows(lang) {
            if context.starts_with('@') {
                continue;
            }
            assert_eq!(source.ends_with('…'), translation.ends_with('…'), "{} line {line}: {source:?} -> {translation:?}", lang.code);
        }
    }
}

/// Every string the UI looks up by literal (`tl!("…")`, `t("…")`) in the language of each catalog
/// that claims to be complete.
#[test]
fn complete_catalogs_cover_every_ui_literal() {
    let sources = [
        include_str!("../app.rs"),
        include_str!("../dialogs.rs"),
        include_str!("../model.rs"),
        include_str!("../notes.rs"),
        include_str!("../settings.rs"),
        include_str!("../views.rs"),
        include_str!("../widgets.rs"),
    ];
    let mut keys = BTreeSet::new();
    for source in sources {
        // `t("…")` calls, not `fmt(`, `.get(` and the like.
        for (i, _) in source.match_indices("t(\"") {
            if source[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '.') {
                continue;
            }
            let start = i + 3;
            if let Some(len) = source[start..].find('"') {
                keys.insert(&source[start..start + len]);
            }
        }
        for (i, _) in source.match_indices("tl!(\"") {
            let start = i + 5;
            if let Some(len) = source[start..].find('"') {
                keys.insert(&source[start..start + len]);
            }
        }
    }
    assert!(keys.len() > 100, "found only {} UI literals", keys.len());
    // Plural keys: `tn(n, "one", "other")`.
    let mut plurals = BTreeSet::new();
    for source in sources {
        for (i, _) in source.match_indices("tn(") {
            if source[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let forms: Vec<&str> = source[i..].split('"').skip(1).step_by(2).take(2).collect();
            if let [one, other] = forms.as_slice() {
                plurals.insert(format!("{one}|{other}"));
            }
        }
    }
    assert!(plurals.len() >= 4, "found only {plurals:?}");
    for lang in Lang::all().filter(|l| l.complete_menus()) {
        let missing: Vec<_> = keys.iter().filter(|k| !super::has(lang, k)).collect();
        assert!(missing.is_empty(), "{} is missing translations for {missing:#?}", lang.code());
        let missing: Vec<_> = plurals.iter().filter(|k| lang.catalog().plural(k, 0).is_none()).collect();
        assert!(missing.is_empty(), "{} is missing @plural rows for {missing:#?}", lang.code());
    }
}

#[test]
fn complete_catalogs_translate_every_tagline() {
    for lang in Lang::all().filter(|l| l.complete_menus()) {
        for app in &artcraft_launcher_core::SUITE {
            assert_ne!(super::tr_ctx(lang, app.id, app.tagline), app.tagline, "{}: {}", lang.code(), app.id);
        }
    }
}

#[test]
fn product_names_stay_in_english() {
    for lang in Lang::all() {
        for name in ["ArtCraft Launcher", "Craft App Suite", "Launcher"] {
            assert_eq!(tr(lang, name), name, "{}", lang.code());
        }
    }
}

#[test]
fn russian_plural_rule_works() {
    for n in [1, 21, 31, 101] {
        assert_eq!(plural_east_slavic(n), 0, "{n}");
    }
    for n in [2, 3, 4, 22, 23, 24, 102] {
        assert_eq!(plural_east_slavic(n), 1, "{n}");
    }
    for n in [0, 5, 11, 12, 13, 14, 15, 100, 111] {
        assert_eq!(plural_east_slavic(n), 2, "{n}");
    }
    assert_eq!(trn(Lang::RU, 21, "{n} day ago", "{n} days ago"), "21 день назад");
    assert_eq!(trn(Lang::RU, 3, "{n} day ago", "{n} days ago"), "3 дня назад");
}

#[test]
fn untranslated_plurals_fall_back_to_english_by_count() {
    // Russian's rule says "one" for 21, but English says "other".
    assert_eq!(trn(Lang::RU, 21, "{n} unknown item", "{n} unknown items"), "21 unknown items");
    assert_eq!(trn(Lang::RU, 1, "{n} unknown item", "{n} unknown items"), "1 unknown item");
    assert_eq!(trn(Lang::EN, 0, "{n} app", "{n} apps"), "0 apps");
}

#[test]
fn locale_tags_resolve() {
    assert_eq!(lang_from_tag("ru_RU.UTF-8"), Some(Lang::RU));
    assert_eq!(lang_from_tag("ru-RU"), Some(Lang::RU));
    assert_eq!(lang_from_tag("en-US"), Some(Lang::EN));
    assert_eq!(lang_from_tag("C"), Some(Lang::EN));
    assert_eq!(lang_from_tag("C.UTF-8"), Some(Lang::EN));
    assert_eq!(lang_from_tag("POSIX"), Some(Lang::EN));
    assert_eq!(lang_from_tag("de-DE"), None);
    assert_eq!(lang_from_tag(""), None);
    assert_eq!(super::detect::first_supported(["de-DE", "ru-RU", "en-US"]), Some(Lang::RU));
}

#[test]
fn language_labels_come_from_the_registry() {
    use crate::settings::Settings;
    for lang in Lang::all() {
        assert_eq!(Settings::language_label(lang.code()), lang.name());
    }
    assert_eq!(Settings::language_label(Settings::LANGUAGE_AUTO), "Auto");
    assert_eq!(Settings::language_label("xx"), "Auto");
}

// The parser.

#[test]
fn parse_rows_of_every_kind() {
    let cat = Catalog::parse(
        "# comment\n\thello\tworld\nmenu\tOpen\tОткрыть\n@id\tfile.open\tОткрыть файл\n@plural\t{n} file|{n} files\t{n} файл|{n} файла|{n} файлов\n\
         @plural\t{n} app|{n} apps\t{n} 个应用\nbroken row\n",
    );
    assert_eq!(cat.plain("hello"), Some("world"));
    assert_eq!(cat.contextual("menu", "Open"), Some("Открыть"));
    assert_eq!(cat.id("file.open"), Some("Открыть файл"));
    assert_eq!(cat.plural("{n} file|{n} files", 0), Some("{n} файл"));
    assert_eq!(cat.plural("{n} file|{n} files", 2), Some("{n} файлов"));
    // One form serves every count.
    assert_eq!(cat.plural("{n} app|{n} apps", 1), Some("{n} 个应用"));
}

#[test]
fn unescape_works() {
    assert_eq!(unescape("hello\\tworld"), "hello\tworld");
    assert_eq!(unescape("line1\\nline2"), "line1\nline2");
    assert_eq!(unescape("backslash\\\\here"), "backslash\\here");
}

#[test]
fn message_templates_match_real_messages() {
    let cat = Catalog::parse(
        "@msg\tDownload cancelled\tСкачивание отменено\n\
         @msg\tCouldn't open {name}: {e}\tНе удалось открыть {name}: {e}\n\
         @msg\t{name} updated to {version}\t{name} обновлено до {version}\n\
         @msg\t{name} updated to {version} (no checksum)\t{name} обновлено до {version} (без суммы)\n\
         @msg\t{a}{b}\tnever\n",
    );
    assert_eq!(cat.message("Download cancelled").as_deref(), Some("Скачивание отменено"));
    assert_eq!(cat.message("Couldn't open PhotoCraft: no such file: x").as_deref(), Some("Не удалось открыть PhotoCraft: no such file: x"));
    assert_eq!(cat.message("PhotoCraft updated to 0.5.0").as_deref(), Some("PhotoCraft обновлено до 0.5.0"));
    // The template that spells out more of the message wins.
    assert_eq!(cat.message("PhotoCraft updated to 0.5.0 (no checksum)").as_deref(), Some("PhotoCraft обновлено до 0.5.0 (без суммы)"));
    // Placeholders never match nothing, and unknown messages aren't touched.
    assert_eq!(cat.message("Couldn't open : x"), None);
    assert_eq!(cat.message("Something else"), None);
    assert_eq!(message(Lang::EN, "Something else"), "Something else");
}

#[test]
fn complete_catalogs_translate_package_formats() {
    use artcraft_launcher_core::PackageKind;
    let kinds =
        [PackageKind::Msi, PackageKind::PortableZip, PackageKind::Dmg, PackageKind::Deb, PackageKind::Rpm, PackageKind::Flatpak, PackageKind::TarGz];
    for lang in Lang::all().filter(|l| l.complete_menus()) {
        for kind in kinds {
            assert!(super::has(lang, kind.label()), "{}: {}", lang.code(), kind.label());
        }
    }
}
