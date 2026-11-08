//! UI translations with Project Fluent. Messages live in `locales/*.ftl` and
//! are embedded at compile time.

use std::sync::LazyLock;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use gpui::{App, SharedString};

use crate::settings::{Language, Settings};

type Bundle = FluentBundle<FluentResource>;

const EN_US_FTL: &str = include_str!("../locales/en-US.ftl");
const ZH_CN_FTL: &str = include_str!("../locales/zh-CN.ftl");

static EN_US: LazyLock<Bundle> = LazyLock::new(|| bundle("en-US", EN_US_FTL));
static ZH_CN: LazyLock<Bundle> = LazyLock::new(|| bundle("zh-CN", ZH_CN_FTL));

/// The supported language matching the system locale: Simplified Chinese
/// for any Chinese locale (`zh-Hans-CN`, `zh_CN.UTF-8`, `zh-TW`, …),
/// English otherwise.
static SYSTEM_LANGUAGE: LazyLock<Language> = LazyLock::new(|| {
    let chinese = sys_locale::get_locale().is_some_and(|locale| {
        locale
            .split(['-', '_', '.', '@'])
            .next()
            .is_some_and(|language| language.eq_ignore_ascii_case("zh"))
    });
    if chinese {
        Language::SimplifiedChinese
    } else {
        Language::English
    }
});

/// The concrete language the settings select: never [`Language::System`].
pub fn current(cx: &App) -> Language {
    match Settings::get(cx).language {
        Language::System => *SYSTEM_LANGUAGE,
        language => language,
    }
}

/// Message `id` in the current language.
pub fn tr(cx: &App, id: &str) -> SharedString {
    translate(current(cx), id, None)
}

/// Message `id` in the current language, with Fluent variables.
pub fn tr_args(cx: &App, id: &str, args: &FluentArgs) -> SharedString {
    translate(current(cx), id, Some(args))
}

/// Format message `id`, falling back to English and then to the id itself
/// when it is missing.
fn translate(language: Language, id: &str, args: Option<&FluentArgs>) -> SharedString {
    let bundle: &Bundle = match language {
        Language::SimplifiedChinese => &ZH_CN,
        Language::English | Language::System => &EN_US,
    };
    format(bundle, id, args)
        .or_else(|| format(&EN_US, id, args))
        .unwrap_or_else(|| {
            eprintln!("missing translation: {id}");
            id.to_owned()
        })
        .into()
}

fn format(bundle: &Bundle, id: &str, args: Option<&FluentArgs>) -> Option<String> {
    let pattern = bundle.get_message(id)?.value()?;
    let mut errors = Vec::new();
    let text = bundle.format_pattern(pattern, args, &mut errors);
    for error in errors {
        eprintln!("translation error in {id}: {error}");
    }
    Some(text.into_owned())
}

fn bundle(locale: &str, source: &str) -> Bundle {
    let resource =
        FluentResource::try_new(source.to_owned()).unwrap_or_else(|(resource, errors)| {
            for error in errors {
                eprintln!("syntax error in {locale}.ftl: {error:?}");
            }
            resource
        });
    let mut bundle = Bundle::new_concurrent(vec![locale.parse().expect("valid locale id")]);
    // Unicode isolation marks around placeables would render as visible
    // glyphs in some fonts.
    bundle.set_use_isolating(false);
    if let Err(errors) = bundle.add_resource(resource) {
        for error in errors {
            eprintln!("error in {locale}.ftl: {error}");
        }
    }
    bundle
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use fluent_bundle::FluentResource;
    use fluent_syntax::ast::Entry;

    use super::{EN_US_FTL, ZH_CN_FTL};

    fn message_ids(locale: &str, source: &str) -> BTreeSet<String> {
        let resource = FluentResource::try_new(source.to_owned())
            .unwrap_or_else(|(_, errors)| panic!("syntax errors in {locale}.ftl: {errors:?}"));
        resource
            .entries()
            .filter_map(|entry| match entry {
                Entry::Message(message) => Some(message.id.name.to_owned()),
                _ => None,
            })
            .collect()
    }

    /// Every locale parses and defines exactly the English messages, so no
    /// language silently falls back to English or carries stale entries.
    #[test]
    fn locales_define_the_same_messages() {
        let english = message_ids("en-US", EN_US_FTL);
        assert_eq!(message_ids("zh-CN", ZH_CN_FTL), english);
    }
}
