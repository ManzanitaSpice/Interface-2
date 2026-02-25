use std::sync::{Mutex, OnceLock};

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

use crate::domain::models::instance::InstanceMetadata;

const DISCORD_APP_ID: &str = "1472001752252289169";
const LOGO_IMAGE_KEY: &str = "logo";

static DISCORD_RPC_CLIENT: OnceLock<Mutex<Option<DiscordIpcClient>>> = OnceLock::new();

fn rpc_client() -> &'static Mutex<Option<DiscordIpcClient>> {
    DISCORD_RPC_CLIENT.get_or_init(|| Mutex::new(None))
}

pub fn initialize_discord_rpc() {
    set_activity(launcher_activity());
}

pub fn set_launcher_presence() {
    set_activity(launcher_activity());
}

pub fn set_instance_presence(metadata: &InstanceMetadata) {
    let details = format!("Jugando Minecraft {}", metadata.minecraft_version);
    let state = if metadata.loader_version.trim().is_empty() {
        metadata.loader.trim().to_string()
    } else {
        format!(
            "{} {}",
            metadata.loader.trim(),
            metadata.loader_version.trim()
        )
    };

    let activity = activity::Activity::new()
        .details(&details)
        .state(&state)
        .assets(
            activity::Assets::new()
                .large_image(LOGO_IMAGE_KEY)
                .large_text(&metadata.name)
                .small_image(LOGO_IMAGE_KEY)
                .small_text("Interface Launcher"),
        );

    set_activity(activity);
}

fn set_activity(activity: activity::Activity) {
    let mut guard = match rpc_client().lock() {
        Ok(guard) => guard,
        Err(_) => {
            log::warn!("No se pudo bloquear Discord RPC client (poisoned lock)");
            return;
        }
    };

    if ensure_connected(&mut guard).is_err() {
        return;
    }

    let Some(client) = guard.as_mut() else {
        return;
    };

    if let Err(err) = client.set_activity(activity) {
        log::warn!("No se pudo actualizar Discord Rich Presence: {err}");
    }
}

fn ensure_connected(guard: &mut Option<DiscordIpcClient>) -> Result<(), ()> {
    if guard.is_some() {
        return Ok(());
    }

    let mut client = match DiscordIpcClient::new(DISCORD_APP_ID) {
        Ok(client) => client,
        Err(err) => {
            log::warn!("No se pudo crear cliente Discord RPC: {err}");
            return Err(());
        }
    };

    if let Err(err) = client.connect() {
        log::warn!("No se pudo conectar con Discord RPC: {err}");
        return Err(());
    }

    *guard = Some(client);
    log::info!("Discord RPC conectado (app_id={DISCORD_APP_ID})");
    Ok(())
}

fn launcher_activity() -> activity::Activity {
    activity::Activity::new()
        .details("En el launcher")
        .state("Seleccionando instancia")
        .assets(
            activity::Assets::new()
                .large_image(LOGO_IMAGE_KEY)
                .large_text("Interface Launcher")
                .small_image(LOGO_IMAGE_KEY)
                .small_text("Interface Launcher"),
        )
}
