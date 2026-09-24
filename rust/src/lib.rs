//! C ABI wrapper around autobase's per-script MinMax mode (`autobase -m -b`),
//! loaded by the All Your BASE Glyphs plugin through ctypes.
//!
//! The measuring logic is ported from autobase-cli's `main.rs`, minus CJK.
//! Every script also gets the ideographic em-box baselines (ideo, idtp) for
//! aligning with CJK fonts.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    ffi::{c_char, CStr, CString},
    fs, iter, panic,
};

use anyhow::Context;
use autobase::{
    base::{BaseScript, BaseTable, MinMax},
    base_script,
    config::Config,
    error::AutobaseError,
    utils::{iso15924_to_opentype, supported_scripts},
};
use fontheight::{Report, Reporter};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use skrifa::{raw::TableProvider, Tag};
use static_lang_word_lists::{WordList, WordListMetadata, ALL_WORD_LISTS};
use write_fonts::{tables::base as write_base, FontBuilder};

#[derive(Deserialize)]
struct Request {
    /// Same keys as autobase's TOML config: override, languages, tolerance, exclusions
    autobase: Config,
    words_per_list: usize,
    extra_words: Vec<ExtraWords>,
    /// HorizAxis.ideo in font units; defaults to 12% of the em below the baseline
    ideographic_bottom: Option<i16>,
}

/// Words from the user's words file, for one script (and optionally one language)
#[derive(Deserialize)]
struct ExtraWords {
    script: String,
    language: Option<String>,
    words: Vec<String>,
}

#[derive(Serialize, Default)]
struct Response {
    written: bool,
    replaced_existing: bool,
    fea: String,
    summary: Vec<String>,
    error: Option<String>,
}

fn generate(font_bytes: &[u8], request: &Request) -> anyhow::Result<BaseTable> {
    let config = &request.autobase;
    let reporter = Reporter::new(font_bytes)?;
    let font = reporter.fontref();
    let locations = reporter.interesting_locations();
    let instances = locations
        .par_iter()
        .map(|location| reporter.instance(location))
        .collect::<Result<Vec<_>, _>>()
        .context("failed to initialise instances for testing")?;
    let supported = supported_scripts(font);

    let extra_word_lists: Vec<WordList> = request
        .extra_words
        .iter()
        .map(|extra| {
            let name = match &extra.language {
                Some(language) => format!("words file {}_{}", language, extra.script),
                None => format!("words file {}", extra.script),
            };
            WordList::define(
                WordListMetadata {
                    name: Cow::Owned(name),
                    script: Some(Cow::Owned(extra.script.clone())),
                    language: extra.language.clone().map(Cow::Owned),
                },
                extra.words.clone(),
            )
        })
        .collect();
    let word_lists = ALL_WORD_LISTS
        .iter()
        .copied()
        .chain(extra_word_lists.iter())
        .filter(|word_list| {
            // Filter out word lists that don't have a script in the font
            word_list
                .script()
                .map(|x| supported.contains(x))
                .unwrap_or(false)
        });
    // Plenty of exemplars, so autobase can skip words that contain exclusions
    let reports = word_lists
        .flat_map(|word_list| instances.iter().zip(iter::repeat(word_list)))
        .par_bridge()
        .map(|(reporter, word_list)| {
            reporter.par_check(word_list, Some(request.words_per_list), 10000)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut reports_by_script: BTreeMap<String, Vec<Report>> = BTreeMap::new();
    for report in reports.into_iter() {
        if let Some(script) = report.word_list.script() {
            reports_by_script
                .entry(script.to_string())
                .or_default()
                .push(report);
        }
    }

    let os2 = font.os2()?;
    let font_minmax = MinMax::new_min_max(os2.s_typo_descender(), os2.s_typo_ascender());
    let base_script_records = reports_by_script
        .iter()
        .flat_map(|(script, reports)| {
            base_script::base_script_record(script, reports, config, &font_minmax)
        })
        .collect::<Vec<_>>();

    let mut base = BaseTable::new(base_script_records, vec![]);
    base.simplify(config.tolerance);

    // Ideographic em-box baselines, so apps can align this font with CJK fonts
    let upem = font.head()?.units_per_em() as f32;
    let ideo = request
        .ideographic_bottom
        .unwrap_or((-0.12 * upem).round() as i16);
    let idtp = ideo + upem as i16;
    for ot_script in supported.iter().flat_map(|s| iso15924_to_opentype(s)) {
        let base_script =
            if let Some(bs) = base.horizontal.iter_mut().find(|bs| bs.script == ot_script) {
                bs
            } else {
                base.horizontal.push(BaseScript::new(ot_script));
                base.horizontal.last_mut().unwrap()
            };
        base_script.default_baseline = Some(Tag::new(b"romn"));
        base_script.baselines.insert(Tag::new(b"romn"), 0);
        base_script.baselines.insert(Tag::new(b"ideo"), ideo);
        base_script.baselines.insert(Tag::new(b"idtp"), idtp);
    }
    Ok(base)
}

/// Like autobase's `BaseTable::to_skrifa`, but the BaseTagList holds every
/// baseline in use, not only the scripts' default baselines. Horizontal only.
fn to_write_fonts(base: &BaseTable) -> Result<write_base::Base, AutobaseError> {
    let baseline_tags: Vec<Tag> = base
        .horizontal
        .iter()
        .flat_map(|script| script.baselines.keys().copied().chain(script.default_baseline))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut scripts = base
        .horizontal
        .iter()
        .map(|script| script.to_skrifa(&baseline_tags))
        .collect::<Result<Vec<_>, _>>()?;
    scripts.sort_by_key(|record| record.base_script_tag);
    let tag_list = (!baseline_tags.is_empty()).then(|| write_base::BaseTagList::new(baseline_tags));
    let horizontal_axis = write_base::Axis::new(tag_list, write_base::BaseScriptList::new(scripts));
    Ok(write_base::Base::new(Some(horizontal_axis), None))
}

fn process(font_path: &str, request_json: &str) -> anyhow::Result<Response> {
    let request: Request =
        serde_json::from_str(request_json).context("invalid plugin configuration")?;
    let font_bytes = fs::read(font_path).context("failed to read font file")?;
    let base = generate(&font_bytes, &request)?;

    let mut response = Response::default();
    if let Some(script) = base.horizontal.first() {
        let baselines: Vec<String> = script
            .baselines
            .iter()
            .map(|(tag, value)| format!("{} {}", tag, value))
            .collect();
        response.summary.push(format!("baselines (all scripts): {}", baselines.join(", ")));
    }
    for script in &base.horizontal {
        if let Some(minmax) = &script.default_minmax {
            response.summary.push(format!("{} dflt {}", script.script, minmax));
        }
        for (language, minmax) in &script.languages {
            response.summary.push(format!("{} {} {}", script.script, language, minmax));
        }
    }
    if base.horizontal.is_empty() {
        // No scripts found in the font, so there is nothing to write
        return Ok(response);
    }

    let font = skrifa::FontRef::new(&font_bytes).context("failed to parse font file")?;
    response.replaced_existing = font.table_data(Tag::new(b"BASE")).is_some();
    let mut builder = FontBuilder::new();
    builder.add_table(&to_write_fonts(&base)?)?;
    builder.copy_missing_tables(font.clone());
    let binary = builder.build();
    fs::write(font_path, binary).context("failed to write font file")?;
    response.written = true;
    response.fea = base.to_fea();
    Ok(response)
}

/// Measures the font at `font_path`, writes a BASE table into it in place and
/// returns a JSON response. Free the result with `autobase_glyphs_free`.
///
/// # Safety
/// Both arguments must be valid, NUL-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn autobase_glyphs_process(
    font_path: *const c_char,
    request_json: *const c_char,
) -> *mut c_char {
    let result = panic::catch_unwind(|| {
        let font_path = CStr::from_ptr(font_path).to_str()?;
        let request_json = CStr::from_ptr(request_json).to_str()?;
        process(font_path, request_json)
    });
    let response = match result {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => Response {
            error: Some(format!("{:#}", error)),
            ..Default::default()
        },
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            Response {
                error: Some(format!("autobase panicked: {}", message)),
                ..Default::default()
            }
        }
    };
    let json = serde_json::to_string(&response).expect("response is serializable");
    CString::new(json).expect("JSON has no NUL bytes").into_raw()
}

/// Frees a string returned by `autobase_glyphs_process`.
///
/// # Safety
/// `s` must come from `autobase_glyphs_process` and be freed only once.
#[no_mangle]
pub unsafe extern "C" fn autobase_glyphs_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}
