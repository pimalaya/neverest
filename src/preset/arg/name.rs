use clap::Parser;

/// The optional preset name argument parser.
#[derive(Debug, Parser)]
pub struct OptionalPresetNameArg {
    /// The name of the preset.
    ///
    /// The preset name corresponds to the name of the TOML table
    /// entry at path `presets.<name>`. If omitted, the preset marked
    /// as default will be used.
    #[arg(name = "preset_name", value_name = "PRESET")]
    pub name: Option<String>,
}
