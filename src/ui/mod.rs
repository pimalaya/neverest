pub(crate) mod prompt;

use dialoguer::theme::ColorfulTheme;
use once_cell::sync::Lazy;

pub(crate) static THEME: Lazy<ColorfulTheme> = Lazy::new(ColorfulTheme::default);
