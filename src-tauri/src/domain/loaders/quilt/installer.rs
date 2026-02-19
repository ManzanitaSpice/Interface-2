pub fn quilt_profile_url(minecraft_version: &str, loader_version: &str) -> String {
    format!(
        "https://meta.quiltmc.org/v3/versions/loader/{minecraft_version}/{loader_version}/profile/json"
    )
}
