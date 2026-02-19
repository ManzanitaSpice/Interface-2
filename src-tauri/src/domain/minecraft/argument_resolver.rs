use std::collections::HashMap;

use serde_json::Value;

use super::rule_engine::{evaluate_rules, RuleContext};

#[derive(Debug, Clone)]
pub struct LaunchContext {
    pub classpath: String,
    pub classpath_separator: String,
    pub library_directory: String,
    pub natives_dir: String,
    pub launcher_name: String,
    pub launcher_version: String,
    pub auth_player_name: String,
    pub auth_uuid: String,
    pub auth_access_token: String,
    pub user_type: String,
    pub user_properties: String,
    pub version_name: String,
    pub game_directory: String,
    pub assets_root: String,
    pub assets_index_name: String,
    pub version_type: String,
    pub resolution_width: String,
    pub resolution_height: String,
    pub clientid: String,
    pub auth_xuid: String,
    pub xuid: String,
    pub quick_play_singleplayer: String,
    pub quick_play_multiplayer: String,
    pub quick_play_realms: String,
    pub quick_play_path: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedLaunchArguments {
    pub main_class: String,
    pub jvm: Vec<String>,
    pub game: Vec<String>,
    pub all: Vec<String>,
}

pub fn resolve_launch_arguments(
    version_json: &Value,
    launch: &LaunchContext,
    rule_context: &RuleContext,
) -> Result<ResolvedLaunchArguments, String> {
    let main_class = version_json
        .get("mainClass")
        .and_then(Value::as_str)
        .ok_or_else(|| "version.json no contiene mainClass".to_string())?
        .to_string();

    let replacements = replacement_map(launch);

    if let Some(arguments) = version_json.get("arguments") {
        let jvm = resolve_argument_section(arguments.get("jvm"), &replacements, rule_context);
        let game = resolve_argument_section(arguments.get("game"), &replacements, rule_context);
        let mut all = Vec::with_capacity(jvm.len() + game.len());
        all.extend(jvm.clone());
        all.extend(game.clone());

        return Ok(ResolvedLaunchArguments {
            main_class,
            jvm,
            game,
            all,
        });
    }

    if let Some(legacy) = version_json
        .get("minecraftArguments")
        .and_then(Value::as_str)
    {
        let game = legacy
            .split_whitespace()
            .map(|item| replace_variables(item, &replacements))
            .collect::<Vec<_>>();

        return Ok(ResolvedLaunchArguments {
            main_class,
            jvm: Vec::new(),
            all: game.clone(),
            game,
        });
    }

    Err("version.json no contiene arguments ni minecraftArguments".to_string())
}

fn resolve_argument_section(
    maybe_section: Option<&Value>,
    replacements: &HashMap<String, String>,
    rule_context: &RuleContext,
) -> Vec<String> {
    let Some(section) = maybe_section.and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut args = Vec::new();
    for item in section {
        match item {
            Value::String(value) => args.push(replace_variables(value, replacements)),
            Value::Object(_) => {
                let rules = item
                    .get("rules")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if !evaluate_rules(&rules, rule_context) {
                    continue;
                }

                if let Some(value) = item.get("value") {
                    match value {
                        Value::String(single) => args.push(replace_variables(single, replacements)),
                        Value::Array(multiple) => {
                            for entry in multiple {
                                if let Some(single) = entry.as_str() {
                                    args.push(replace_variables(single, replacements));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    args
}

pub fn replace_launch_variables(raw: &str, launch: &LaunchContext) -> String {
    let replacements = replacement_map(launch);
    replace_variables(raw, &replacements)
}

fn replacement_map(launch: &LaunchContext) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("classpath".to_string(), launch.classpath.clone());
    map.insert(
        "classpath_separator".to_string(),
        launch.classpath_separator.clone(),
    );
    map.insert(
        "library_directory".to_string(),
        launch.library_directory.clone(),
    );
    map.insert("natives_directory".to_string(), launch.natives_dir.clone());
    map.insert("launcher_name".to_string(), launch.launcher_name.clone());
    map.insert(
        "launcher_version".to_string(),
        launch.launcher_version.clone(),
    );
    map.insert(
        "auth_player_name".to_string(),
        launch.auth_player_name.clone(),
    );
    map.insert("auth_uuid".to_string(), launch.auth_uuid.clone());
    map.insert(
        "auth_access_token".to_string(),
        launch.auth_access_token.clone(),
    );
    map.insert("user_type".to_string(), launch.user_type.clone());
    map.insert(
        "user_properties".to_string(),
        launch.user_properties.clone(),
    );
    map.insert("version_name".to_string(), launch.version_name.clone());
    map.insert("game_directory".to_string(), launch.game_directory.clone());
    map.insert("assets_root".to_string(), launch.assets_root.clone());
    map.insert(
        "assets_index_name".to_string(),
        launch.assets_index_name.clone(),
    );
    map.insert("version_type".to_string(), launch.version_type.clone());
    map.insert(
        "resolution_width".to_string(),
        launch.resolution_width.clone(),
    );
    map.insert(
        "resolution_height".to_string(),
        launch.resolution_height.clone(),
    );
    map.insert("clientid".to_string(), launch.clientid.clone());
    map.insert("auth_xuid".to_string(), launch.auth_xuid.clone());
    map.insert("xuid".to_string(), launch.xuid.clone());
    map.insert(
        "quickPlaySingleplayer".to_string(),
        launch.quick_play_singleplayer.clone(),
    );
    map.insert(
        "quickPlayMultiplayer".to_string(),
        launch.quick_play_multiplayer.clone(),
    );
    map.insert(
        "quickPlayRealms".to_string(),
        launch.quick_play_realms.clone(),
    );
    map.insert("quickPlayPath".to_string(), launch.quick_play_path.clone());

    map.insert("username".to_string(), launch.auth_player_name.clone());
    map.insert("uuid".to_string(), launch.auth_uuid.clone());
    map.insert("accessToken".to_string(), launch.auth_access_token.clone());
    map.insert("gameDir".to_string(), launch.game_directory.clone());
    map.insert("assetsDir".to_string(), launch.assets_root.clone());
    map.insert("assetIndex".to_string(), launch.assets_index_name.clone());
    map.insert("game_assets".to_string(), launch.assets_root.clone());

    map
}

fn replace_variables(raw: &str, replacements: &HashMap<String, String>) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut cursor = raw;

    while let Some(start) = cursor.find("${") {
        output.push_str(&cursor[..start]);
        let variable_start = start + 2;
        let candidate = &cursor[variable_start..];

        if let Some(end) = candidate.find('}') {
            let key = &candidate[..end];
            if let Some(value) = replacements.get(key) {
                output.push_str(value);
            } else {
                output.push_str("${");
                output.push_str(key);
                output.push('}');
            }
            cursor = &candidate[end + 1..];
        } else {
            output.push_str(&cursor[start..]);
            return output;
        }
    }

    output.push_str(cursor);
    output
}

pub fn unresolved_variables_in_args<'a>(args: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut unresolved = Vec::new();

    for arg in args {
        unresolved.extend(extract_unresolved_variables(arg));
    }

    unresolved.sort();
    unresolved.dedup();
    unresolved
}

fn extract_unresolved_variables(arg: &str) -> Vec<String> {
    let mut variables = Vec::new();
    let mut cursor = arg;

    while let Some(start) = cursor.find("${") {
        let variable_start = start + 2;
        let candidate = &cursor[variable_start..];

        if let Some(end) = candidate.find('}') {
            variables.push(candidate[..end].to_string());
            cursor = &candidate[end + 1..];
        } else {
            break;
        }
    }

    variables
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::minecraft::rule_engine::OsName;

    fn sample_launch_context() -> LaunchContext {
        LaunchContext {
            classpath: "CP".to_string(),
            classpath_separator: ":".to_string(),
            library_directory: "/libraries".to_string(),
            natives_dir: "NAT".to_string(),
            launcher_name: "Launcher".to_string(),
            launcher_version: "1.0".to_string(),
            auth_player_name: "Steve".to_string(),
            auth_uuid: "uuid-123".to_string(),
            auth_access_token: "token".to_string(),
            user_type: "msa".to_string(),
            user_properties: "{}".to_string(),
            version_name: "1.21.1".to_string(),
            game_directory: "/game".to_string(),
            assets_root: "/assets".to_string(),
            assets_index_name: "17".to_string(),
            version_type: "release".to_string(),
            resolution_width: "1280".to_string(),
            resolution_height: "720".to_string(),
            clientid: "client-id".to_string(),
            auth_xuid: "auth-xuid".to_string(),
            xuid: "xuid".to_string(),
            quick_play_singleplayer: String::new(),
            quick_play_multiplayer: String::new(),
            quick_play_realms: String::new(),
            quick_play_path: String::new(),
        }
    }

    #[test]
    fn resolve_modern_arguments_with_rules() {
        let version_json = json!({
          "mainClass":"net.minecraft.client.main.Main",
          "arguments": {
            "jvm": [
              "-Djava.library.path=${natives_directory}",
              {"rules":[{"action":"allow","os":{"name":"linux"}}],"value":"-Dos=linux"}
            ],
            "game": ["--username", "${auth_player_name}"]
          }
        });

        let result = resolve_launch_arguments(
            &version_json,
            &sample_launch_context(),
            &RuleContext {
                os_name: OsName::Linux,
                arch: "x86_64".to_string(),
                features: RuleFeatures::default(),
            },
        )
        .expect("debe resolver");

        assert_eq!(result.jvm[0], "-Djava.library.path=NAT");
        assert_eq!(result.jvm[1], "-Dos=linux");
        assert_eq!(result.game[1], "Steve");
        assert_eq!(result.main_class, "net.minecraft.client.main.Main");
    }

    #[test]
    fn resolve_modern_arguments_with_classpath_separator_and_library_directory() {
        let version_json = json!({
          "mainClass":"net.minecraft.client.main.Main",
          "arguments": {
            "jvm": [
              "-Dseparator=${classpath_separator}",
              "-Dlibrary=${library_directory}"
            ],
            "game": []
          }
        });

        let result = resolve_launch_arguments(
            &version_json,
            &sample_launch_context(),
            &RuleContext {
                os_name: OsName::Linux,
                arch: "x86_64".to_string(),
                features: RuleFeatures::default(),
            },
        )
        .expect("debe resolver");

        assert_eq!(result.jvm[0], "-Dseparator=:");
        assert_eq!(result.jvm[1], "-Dlibrary=/libraries");
    }

    #[test]
    fn resolve_legacy_arguments() {
        let version_json = json!({
          "mainClass":"net.minecraft.client.main.Main",
          "minecraftArguments":"--username ${username} --gameDir ${gameDir}"
        });

        let result = resolve_launch_arguments(
            &version_json,
            &sample_launch_context(),
            &RuleContext {
                os_name: OsName::Windows,
                arch: "x86_64".to_string(),
                features: RuleFeatures::default(),
            },
        )
        .expect("debe resolver");

        assert!(result.jvm.is_empty());
        assert_eq!(
            result.game,
            vec!["--username", "Steve", "--gameDir", "/game"]
        );
    }

    #[test]
    fn resolve_modern_optional_placeholders_for_resolution_and_quickplay() {
        let version_json = json!({
          "mainClass":"net.minecraft.client.main.Main",
          "arguments": {
            "jvm": [
              "-Dwidth=${resolution_width}",
              "-Dheight=${resolution_height}",
              "-Dclientid=${clientid}",
              "-Dxuid=${auth_xuid}",
              "-Dquick=${quickPlayPath}"
            ],
            "game": []
          }
        });

        let result = resolve_launch_arguments(
            &version_json,
            &sample_launch_context(),
            &RuleContext {
                os_name: OsName::Linux,
                arch: "x86_64".to_string(),
                features: RuleFeatures::default(),
            },
        )
        .expect("debe resolver placeholders opcionales");

        assert_eq!(result.jvm[0], "-Dwidth=1280");
        assert_eq!(result.jvm[1], "-Dheight=720");
        assert_eq!(result.jvm[2], "-Dclientid=client-id");
        assert_eq!(result.jvm[3], "-Dxuid=auth-xuid");
        assert_eq!(result.jvm[4], "-Dquick=");
    }

    #[test]
    fn unresolved_variables_are_reported_by_name() {
        let args = vec![
            "-Dfoo=${launcher_name}".to_string(),
            "--token=${auth_access_token}".to_string(),
            "--broken=${not_closed".to_string(),
            "--multi=${version_name}:${version_type}".to_string(),
        ];

        let unresolved = unresolved_variables_in_args(args.iter());

        assert_eq!(
            unresolved,
            vec![
                "auth_access_token".to_string(),
                "launcher_name".to_string(),
                "version_name".to_string(),
                "version_type".to_string(),
            ]
        );
    }
}
