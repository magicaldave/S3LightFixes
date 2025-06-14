use std::{
    env::{current_dir, var},
    fs::{File, create_dir_all, metadata, read_dir, read_to_string, remove_file},
    io::{self, Write},
    path::{Path, PathBuf},
    process::exit,
};

use clap::Parser;

use openmw_config::OpenMWConfiguration;
use palette::{FromColor, Hsv, IntoColor, rgb::Srgb};
use serde::{Deserialize, Serialize};
use tes3::esp::*;
use vfstool_lib::VFS;

const DEFAULT_CONFIG_NAME: &str = "lightconfig.toml";
const LOG_NAME: &str = "lightconfig.log";
const PLUGIN_NAME: &str = "S3LightFixes.omwaddon";

#[derive(Parser, Debug)]
#[command(
    name = "S3 Lightfixes",
    about = "A tool for modifying light values globally across an OpenMW installation.\nPlease note that arguments provided here, which also exist in lightConfig.toml, will override any values in lightConfig.toml when used.\nAdditionally, if the lightConfig.toml does not exist, the used values will be saved into the new lightConfig.toml."
)]
struct LightArgs {
    /// Path to openmw.cfg
    /// By default, uses the system paths defined by:
    ///
    /// https://openmw.readthedocs.io/en/latest/reference/modding/paths.html
    ///
    /// Alternatively, responds to both the `OPENMW_CONFIG` and `OPENMW_CONFIG_DIR`
    /// environment variables.
    #[arg(short = 'c', long = "openmw-cfg")]
    openmw_cfg: Option<PathBuf>,

    /// Enables classic mode using vtastek shaders.
    /// ONLY for openmw 0.47. Relevant shaders can be found in the OpenMW discord:
    ///
    /// https://discord.com/channels/260439894298460160/718892786157617163/966468825321177148
    #[arg(short = '7', long = "classic")]
    use_classic: bool,

    /// Output file path.
    /// Accepts relative and absolute terms.
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,

    /// Whether to save a text form of the generated plugin.
    /// Extremely verbose!
    ///
    /// You probably don't want to enable this unless asked specifically to do so.
    #[arg(short = 'l', long = "write-log")]
    write_log: bool,

    /// Whether to automatically enable the output plugin in openmw.cfg.
    /// Disabled by default, and only available via CLI.
    ///
    /// Typically lightfixes is ran under momw-configurator, making this param
    /// unnecessary for many users.
    #[arg(short = 'e', long = "auto-enable")]
    auto_enable: bool,

    /// If used, print to stdout instead of using native GUI dialogs.
    ///
    /// Not available on android.
    #[arg(short = 'n', long = "no-notifications")]
    no_notifications: bool,

    /// Output debugging information during lightfixes generation
    /// Primarily displays output related to the openmw.cfg being used for generation
    #[arg(short = 'd', long = "debug")]
    debug: bool,

    /// Outputs version info
    // Might be more later?
    #[arg(short = 'i', long = "info")]
    info: bool,

    /// Whether to disable flickering lights during lightfixes generation
    #[arg(short = 'f', long = "no-flicker")]
    disable_flickering: Option<bool>,

    #[arg(
        long = "standard-hue",
        help = &format!("For lights in the orange range, multiply their HSV hue by this value.\nIf this argument is not used, the value will be derived from lightConfig.toml or use the default value of {}.\nThis argument has no short form due to a conflict with -h.", default::standard_hue())
    )]
    standard_hue: Option<f32>,

    #[arg(
        short = 's',
        long = "standard-saturation",
        help = &format!("For lights in the orange range, multiply their HSV saturation by this amount.\nIf this argument is not used, the value will be derived from lightConfig.toml or use the default value of {}.", default::standard_saturation())
    )]
    standard_saturation: Option<f32>,

    #[arg(
        short = 'v',
        long = "standard-value",
        help = &format!("For lights in the orange range, multiply their HSV value by this amount.\nIf this argument is not used, the value will be derived from lightConfig.toml or use the default value of {}.", default::standard_value())
    )]
    standard_value: Option<f32>,

    #[arg(
        short = 'r',
        long = "standard-radius",
        help = &format!("For lights in the orange range, multiply their radius by this value.\nIf this argument is not used, the value will be derived from lightConfig.toml or use the default value of {}.", default::standard_radius())
    )]
    standard_radius: Option<f32>,

    #[arg(
        short = 'H',
        long = "colored-hue",
        help = &format!("For lights that are red, purple, blue, green, or yellow, multiply their HSV hue by this value.\nIf this argument is not used, the value will be derived from lightConfig.toml or use the default value of {}.", default::colored_hue())
    )]
    colored_hue: Option<f32>,

    #[arg(
        short = 'S',
        long = "colored-saturation",
        help = &format!("For lights that are red, purple, blue, green, or yellow, multiply their HSV saturation by this amount.\nIf this argument is not used, the value will be derived from lightConfig.toml or use the default value of {}.", default::colored_saturation())
    )]
    colored_saturation: Option<f32>,

    #[arg(
        short = 'V',
        long = "colored-value",
        help = &format!("For lights that are red, purple, blue, green, or yellow, multiply their HSV value by this amount.\nIf this argument is not used, the value will be derived from lightConfig.toml or use the default value of {}.", default::colored_value())
    )]
    colored_value: Option<f32>,

    #[arg(
        short = 'R',
        long = "colored-radius",
        help = &format!("For lights that are red, purple, blue, green, or yellow, multiply their radius by this value.\nIf this argument is not used, the value will be derived from lightConfig.toml or use the default value of {}.", default::colored_radius())
    )]
    colored_radius: Option<f32>,
}

mod default {
    pub fn standard_hue() -> f32 {
        0.62
    }

    pub fn standard_saturation() -> f32 {
        0.8
    }

    pub fn standard_value() -> f32 {
        0.57
    }

    /// Original default radius was 2.0
    /// But was only appropriate for vtastek shaders
    /// MOMW configs use 1.2
    pub fn standard_radius() -> f32 {
        1.2
    }

    pub fn colored_hue() -> f32 {
        1.0
    }

    pub fn colored_saturation() -> f32 {
        0.9
    }

    pub fn colored_value() -> f32 {
        0.7
    }

    pub fn colored_radius() -> f32 {
        1.1
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct LightConfig {
    /// This parameter is DANGEROUS
    /// It's only meant to be used with vtastek's experimental shaders for openmw 0.47
    /// <https://discord.com/channels/260439894298460160/718892786157617163/966468825321177148>
    #[serde(skip)]
    disable_interior_sun: bool,
    disable_flickering: bool,
    save_log: bool,

    #[serde(default = "default::standard_hue")]
    standard_hue: f32,

    #[serde(default = "default::standard_saturation")]
    standard_saturation: f32,

    #[serde(default = "default::standard_value")]
    standard_value: f32,

    #[serde(default = "default::standard_radius")]
    standard_radius: f32,

    #[serde(default = "default::colored_hue")]
    colored_hue: f32,

    #[serde(default = "default::colored_saturation")]
    colored_saturation: f32,

    #[serde(default = "default::colored_value")]
    colored_value: f32,

    #[serde(default = "default::colored_radius")]
    colored_radius: f32,
}

/// Primarily exists to provide default implementations
/// for field values
impl LightConfig {
    fn find(root_path: &PathBuf) -> Result<PathBuf, io::Error> {
        read_dir(root_path)?
            .filter_map(|entry| entry.ok())
            .find(|entry| entry.file_name().eq_ignore_ascii_case(DEFAULT_CONFIG_NAME))
            .map(|entry| entry.path())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Light config not found"))
    }

    /// Gives back the lightconfig adjacent to openmw.cfg when called
    /// use_classic dictates whether or not a fixed radius of 2.0 will be used on orange-y lights
    /// and whether or not to disable interior sunlight
    /// the latter field is not de/serializable and can only be used via the --classic argument
    pub fn get(
        light_args: &LightArgs,
        openmw_config: &OpenMWConfiguration,
    ) -> Result<LightConfig, io::Error> {
        let mut write_config = false;

        let mut light_config: LightConfig =
            if let Ok(config_path) = Self::find(&openmw_config.user_config_path()) {
                let config_contents = read_to_string(config_path)?;

                toml::from_str(&config_contents).map_err(to_io_error)?
            } else {
                write_config = true;
                LightConfig::default()
            };

        // Replace any values provided as CLI args in the config
        // use_classic will always override the standard_radius and disable_interior_sun
        [
            (&mut light_config.standard_hue, light_args.standard_hue),
            (
                &mut light_config.standard_saturation,
                light_args.standard_saturation,
            ),
            (&mut light_config.standard_value, light_args.standard_value),
            (
                &mut light_config.standard_radius,
                light_args.standard_radius,
            ),
            (&mut light_config.colored_hue, light_args.colored_hue),
            (
                &mut light_config.colored_saturation,
                light_args.colored_saturation,
            ),
            (&mut light_config.colored_value, light_args.colored_value),
            (&mut light_config.colored_radius, light_args.colored_radius),
        ]
        .iter_mut()
        .for_each(|(field, value)| {
            if let Some(v) = value {
                **field = std::mem::take(v);
            }
        });

        if let Some(status) = light_args.disable_flickering {
            light_config.disable_flickering = status
        }

        // This parameter indicates whether the user requested
        // To use compatibility mode for vtastek's old 0.47 shaders
        // via startup arguments
        // Drastically increases light radii
        // and disables interior sunlight
        if light_args.use_classic {
            light_config.standard_radius = 2.0;
            light_config.disable_interior_sun = true;
        }

        // If the configuration file didn't exist when we tried to find it,
        // serialize it here
        if write_config {
            let config_serialized = toml::to_string_pretty(&light_config).map_err(to_io_error)?;
            let config_path = openmw_config.user_config_path().join(DEFAULT_CONFIG_NAME);
            let mut config_file = File::create(config_path)?;
            write!(config_file, "{}", config_serialized)?;
        }

        Ok(light_config)
    }
}

impl Default for LightConfig {
    fn default() -> LightConfig {
        LightConfig {
            disable_interior_sun: false,
            disable_flickering: true,
            save_log: false,
            standard_hue: default::standard_hue(),
            standard_saturation: default::standard_saturation(),
            standard_value: default::standard_value(),
            standard_radius: default::standard_radius(),
            colored_hue: default::colored_hue(),
            colored_saturation: default::colored_saturation(),
            colored_value: default::colored_value(),
            colored_radius: default::colored_radius(),
        }
    }
}

fn to_io_error<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err.to_string())
}

/// Displays a notification taking title and message as argument
fn notification_box(title: &str, message: &str, no_notifications: bool) {
    #[cfg(target_os = "android")]
    println!("{}", message);

    #[cfg(not(target_os = "android"))]
    if !no_notifications {
        let _ = native_dialog::DialogBuilder::message()
            .set_title(title)
            .set_text(message)
            .alert()
            .show();
    } else {
        println!("{}", message);
    }
}

fn is_fixable_plugin(plug_path: &Path) -> bool {
    // If path doesn't exist
    if metadata(plug_path).is_err() {
        return false;
    // If path is the lightfixes plugin
    } else if plug_path.to_string_lossy().contains(PLUGIN_NAME) {
        return false;
    } else {
        // Don't match extensionless files
        // And also do the match in case-insensitive fashion
        match plug_path.extension() {
            None => return false,
            Some(ext) => match ext.to_ascii_lowercase().to_str().unwrap_or_default() {
                "esp" | "esm" | "omwaddon" | "omwgame" => return true,
                _ => return false,
            },
        }
    }
}

fn save_plugin(output_dir: &PathBuf, generated_plugin: &mut Plugin) -> io::Result<()> {
    let mut plugin_path = output_dir.join(PLUGIN_NAME);

    match metadata(output_dir) {
        Ok(metadata) if !metadata.is_dir() => {
            let cwd =
                current_dir().expect("CRITICAL FAILURE: FAILED TO READ CURRENT WORKING DIRECTORY!");

            eprintln!(
                "WARNING: Couldn't use {} as an output directory, as it isn't a directory. Using the current working directory, {}, instead!",
                output_dir.display(),
                cwd.display()
            );

            plugin_path = cwd.join(PLUGIN_NAME);
        }
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            create_dir_all(output_dir)?;
        }
        Err(err) => return Err(err),
    }

    generated_plugin.save_path(plugin_path)?;

    Ok(())
}

/// Add another parameter to the light args which can specify an absolute path to the full config
fn get_config_dir(args: &mut LightArgs) -> PathBuf {
    if let Some(path) = args.openmw_cfg.take() {
        if path.is_dir() && path.join("openmw.cfg").is_file() {
            return path;
        }
    } else {
        let cwd = current_dir().expect("Failed to get current directory");
        if cwd.join("openmw.cfg").is_file() {
            return cwd;
        }
    }

    openmw_config::default_config_path()
}

fn main() -> io::Result<()> {
    let mut args = LightArgs::parse();

    if args.info {
        println!("S3LightFixes Version: {}", env!("CARGO_PKG_VERSION"),);
        exit(0);
    };

    let no_notifications = var("S3L_NO_NOTIFICATIONS").is_ok() || args.no_notifications;

    let config_dir = get_config_dir(&mut args);

    let output_dir = match args.output {
        Some(ref dir) => dir,
        None => {
            &current_dir().expect("[ CRITICAL FAILURE ]: FAILED TO READ CURRENT WORKING DIRECTORY!")
        }
    };

    // If the openmw.cfg path is provided by the user, force the crate to use
    // whatever they've provided
    let mut config = match openmw_config::OpenMWConfiguration::new(Some(config_dir)) {
        Ok(config) => config,
        Err(error) => {
            let config_fail = format!("{} {:#?}!", "Failed to read openmw.cfg from", error);

            notification_box(
                &"Failed to read configuration file!",
                &config_fail,
                no_notifications,
            );

            exit(127);
        }
    };

    let use_debug = var("S3L_DEBUG").is_ok() || args.debug;

    if use_debug {
        dbg!(&args, &config.root_config(), &config);
    }

    assert!(
        config.content_files().len() > 0,
        "No plugins were found in openmw.cfg! No lights to fix!"
    );

    let light_config = LightConfig::get(&args, &config)?;

    let mut generated_plugin = Plugin::new();
    let mut used_ids: Vec<String> = Vec::new();

    let mut header = Header {
        version: 1.3,
        author: FixedString("S3".to_string()),
        description: FixedString("Plugin generated by s3-lightfixes".to_string()),
        file_type: types::FileType::Esp,
        flags: ObjectFlags::default(),
        num_objects: 0,
        masters: Vec::new(),
    };

    let vfs = VFS::from_directories(config.data_directories(), None);

    let mut used_objects = 0;
    for plugin_name in config.content_files().iter().rev() {
        let plugin_path = match vfs.get_file(plugin_name) {
            Some(plugin) => {
                let maybe_valid_path = plugin.path();

                match is_fixable_plugin(maybe_valid_path) {
                    true => maybe_valid_path,
                    false => continue,
                }
            }
            None => continue,
        };

        let mut plugin = match Plugin::from_path(plugin_path) {
            Ok(plugin) => plugin,
            Err(e) => {
                eprintln!(
                    "[ WARNING ]: Plugin {}: could not be loaded due to error: {}. Continuing light fixes without this mod .  . . Everything will be okay. Yes, it's still working.\n",
                    plugin_path.display(),
                    e
                );
                continue;
            }
        };

        // Disable sunlight color for true interiors
        // Only do this for `classic` mode
        if light_config.disable_interior_sun {
            for cell in plugin.objects_of_type_mut::<Cell>() {
                let cell_id = cell.editor_id_ascii_lowercase().to_string();

                if !cell.data.flags.contains(CellFlags::IS_INTERIOR) || used_ids.contains(&cell_id)
                {
                    continue;
                };

                cell.references.clear();
                // Take the cell for ourselves instead of cloning it
                let mut owned_cell = cell.clone();

                if let Some(mut atmosphere) = owned_cell.atmosphere_data.take() {
                    atmosphere.sunlight_color = [0, 0, 0, 0];
                    owned_cell.atmosphere_data = Some(atmosphere);
                }

                generated_plugin.objects.push(TES3Object::Cell(owned_cell));
                used_ids.push(cell_id);
                used_objects += 1;
            }
        }

        for light in plugin.objects_of_type_mut::<Light>() {
            let light_id = light.editor_id_ascii_lowercase().to_string();
            if used_ids.contains(&light_id) {
                continue;
            }

            let mut light = light.clone();

            if light_config.disable_flickering {
                light
                    .data
                    .flags
                    .remove(LightFlags::FLICKER | LightFlags::FLICKER_SLOW);
            }

            if light.data.flags.contains(LightFlags::NEGATIVE) {
                light.data.flags.remove(LightFlags::NEGATIVE);
                light.data.radius = 0;
                light.data.color = [0, 0, 0, 0];
            } else {
                let light_as_rgb = Srgb::new(
                    light.data.color[0],
                    light.data.color[1],
                    light.data.color[2],
                )
                .into_format();

                let mut light_as_hsv: Hsv = Hsv::from_color(light_as_rgb);
                let light_hue = light_as_hsv.hue.into_degrees();

                let (radius, hue, saturation, value) = match light_hue > 64. || light_hue < 14. {
                    // Red, purple, blue, green, yellow
                    true => (
                        light_config.colored_radius,
                        light_config.colored_hue,
                        light_config.colored_saturation,
                        light_config.colored_value,
                    ),
                    // Everything else
                    false => (
                        light_config.standard_radius,
                        light_config.standard_hue,
                        light_config.standard_saturation,
                        light_config.standard_value,
                    ),
                };

                light.data.radius = (radius * light.data.radius as f32) as u32;
                light_as_hsv = Hsv::new(
                    light_hue * hue,
                    light_as_hsv.saturation * saturation,
                    light_as_hsv.value * value,
                );

                let rgb8_color: Srgb<u8> =
                    <Hsv as IntoColor<Srgb>>::into_color(light_as_hsv).into_format();

                light.data.color = [rgb8_color.red, rgb8_color.green, rgb8_color.blue, 0];
            }

            generated_plugin.objects.push(TES3Object::Light(light));
            used_ids.push(light_id);
            used_objects += 1;
        }

        if used_objects > 0 {
            let plugin_size = metadata(plugin_path)?.len();
            let plugin_string = plugin_path
                .file_name()
                .expect("Plugins must exist to be loaded by openmw-cfg crate!")
                .to_string_lossy()
                .to_string();
            header.masters.insert(0, (plugin_string, plugin_size));

            used_objects = 0;
        }
    }

    if use_debug {
        dbg!(&header);
    }

    assert!(
        header.masters.len() > 0,
        "The generated plugin was not found to have any master files! It's empty! Try running lightfixes again using the S3L_DEBUG environment variable"
    );

    header.num_objects = used_objects;
    generated_plugin.objects.push(TES3Object::Header(header));
    generated_plugin.sort_objects();

    // If the old plugin format exists, remove it
    // Do it before serializing the new plugin, as the target dir may still be the old one
    let legacy_path = Path::new(&config.user_config_path()).join(PLUGIN_NAME);
    if metadata(&legacy_path).is_ok() {
        remove_file(legacy_path)?;
    };

    save_plugin(&output_dir, &mut generated_plugin)?;

    // Handle this arg via clap
    if args.auto_enable {
        let already_enabled = config.content_files().contains(&PLUGIN_NAME.to_string());
        if !already_enabled {
            if config.root_config() == &config.user_config_path() {
                let mut new_files = config.content_files().clone();
                new_files.push(PLUGIN_NAME.to_string());

                config.set_content_files(new_files);
                config.save().expect("Lightfixes plugin failed to save!");
            } else {
                let mut user_config = OpenMWConfiguration::new(Some(config.user_config_path()))
                    .expect("Failed to read user openmw.cfg!");

                let mut new_files = user_config.content_files().clone();
                new_files.push(PLUGIN_NAME.to_string());

                user_config.set_content_files(new_files);
                user_config
                    .save()
                    .expect("Lightfixes plugin failed to save!");
            }
        }
    }

    if args.write_log || light_config.save_log {
        let path = config.user_config_path().join(LOG_NAME);
        let mut file = File::create(path)?;
        let _ = write!(file, "{}", format!("{:#?}", &generated_plugin));
    }

    let lights_fixed = format!(
        "S3LightFixes.omwaddon generated, enabled, and saved in {}",
        output_dir.display()
    );

    notification_box(&"Lightfixes successful!", &lights_fixed, no_notifications);

    Ok(())
}
