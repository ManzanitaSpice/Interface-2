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

    if let Some(arguments) = version_json.get("arguments") {
        let jvm = resolve_argument_section(arguments.get("jvm"), launch, rule_context);
        let game = resolve_argument_section(arguments.get("game"), launch, rule_context);
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

    let game = parse_legacy_minecraft_arguments(version_json, launch);
    if !game.is_empty() {
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
    launch: &LaunchContext,
    rule_context: &RuleContext,
) -> Vec<String> {
    let Some(section) = maybe_section.and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut args = Vec::new();
    for item in section {
        match item {
            Value::String(value) => args.push(replace_launch_variables(value, launch)),
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
                        Value::String(single) => {
                            args.push(replace_launch_variables(single, launch))
                        }
                        Value::Array(multiple) => {
                            for entry in multiple {
                                if let Some(single) = entry.as_str() {
                                    args.push(replace_launch_variables(single, launch));
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
    let replacements: [(&str, &str); 32] = [
        ("${auth_player_name}", &launch.auth_player_name),
        ("${auth_uuid}", &launch.auth_uuid),
        ("${auth_access_token}", &launch.auth_access_token),
        ("${auth_session}", &launch.auth_access_token),
        ("${user_type}", &launch.user_type),
        ("${user_properties}", &launch.user_properties),
        ("${version_name}", &launch.version_name),
        ("${version_type}", &launch.version_type),
        ("${game_directory}", &launch.game_directory),
        ("${assets_root}", &launch.assets_root),
        ("${game_assets}", &launch.assets_root),
        ("${assets_index_name}", &launch.assets_index_name),
        ("${natives_directory}", &launch.natives_dir),
        ("${launcher_name}", &launch.launcher_name),
        ("${launcher_version}", &launch.launcher_version),
        ("${classpath}", &launch.classpath),
        ("${library_directory}", &launch.library_directory),
        ("${classpath_separator}", &launch.classpath_separator),
        ("${resolution_width}", &launch.resolution_width),
        ("${resolution_height}", &launch.resolution_height),
        ("${clientid}", &launch.clientid),
        ("${auth_xuid}", &launch.auth_xuid),
        ("${quickPlayPath}", &launch.quick_play_path),
        ("${quickPlaySingleplayer}", &launch.quick_play_singleplayer),
        ("${quickPlayMultiplayer}", &launch.quick_play_multiplayer),
        ("${quickPlayRealms}", &launch.quick_play_realms),
        ("${username}", &launch.auth_player_name),
        ("${uuid}", &launch.auth_uuid),
        ("${accessToken}", &launch.auth_access_token),
        ("${gameDir}", &launch.game_directory),
        ("${assetsDir}", &launch.assets_root),
        ("${assetIndex}", &launch.assets_index_name),
    ];

    let mut result = raw.to_string();
    for (placeholder, value) in replacements {
        result = result.replace(placeholder, value);
    }

    result
}

pub fn parse_legacy_minecraft_arguments(
    version_json: &Value,
    context: &LaunchContext,
) -> Vec<String> {
    let raw = match version_json
        .get("minecraftArguments")
        .and_then(Value::as_str)
    {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Vec::new(),
    };

    raw.split_whitespace()
        .map(|token| replace_launch_variables(token, context))
        .filter(|token| !token.trim().is_empty())
        .collect()
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
