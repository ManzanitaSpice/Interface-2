pub fn fabric_profile_url(minecraft_version: &str, loader_version: &str) -> String {
    format!(
        "https://meta.fabricmc.net/v2/versions/loader/{minecraft_version}/{loader_version}/profile/json"
    )
}
