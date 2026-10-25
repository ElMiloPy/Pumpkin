#[allow(clippy::wildcard_imports)]
use super::*;

impl JavaClient {
    pub fn handle_player_ground(&self, player: &Player, ground: &SSetPlayerGround) {
        // A movement packet was received this tick — tracked for SClientTickEnd zeroing.
        self.received_movement_this_tick
            .store(true, Ordering::Relaxed);
        player
            .living_entity
            .entity
            .on_ground
            .store(ground.on_ground, Ordering::Relaxed);

        if !player
            .abilities
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .flying
            && player.living_entity.health.load() > 0.0
            && !player.living_entity.dead.load(Ordering::Relaxed)
        {
            player.living_entity.fall(
                player,
                0.0,
                ground.on_ground,
                player.gamemode.load() == GameMode::Creative,
            );
        }
    }
}
