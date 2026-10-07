//! Delayed sounds (docs/11-audio.md, "Delayed playback").
//!
//! A play request with mode 2 or more doesn't sound at once: the play
//! function (0x15CA9) stores it in one of 8 slots and schedules timeline
//! event 0x15 for `mode - 1` ticks later, with the slot number in the
//! event's x/y bytes. When the event runs (0x160DB) the slot is replayed as
//! an ordinary positional request (mode 1) if its map is still the party's
//! map, and the slot is freed either way. Thunder uses this to arrive a few
//! ticks after the flash.

use crate::effects::Effect;
use crate::state::GameState;
use crate::timeline::Event;

/// Timeline event type that plays a delayed sound.
pub const EV_DELAYED_SOUND: u8 = 0x15;
/// Slots in the delayed-sound table (0x7F188).
pub const SLOTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayedSound {
    pub cat: u8,
    pub idx: u8,
    pub sub: u8,
    pub vol: u8,
    pub map: usize,
    pub x: i32,
    pub y: i32,
}

/// Ask for a sound with the play function's mode. Mode 2 or more is held
/// in a slot and replayed by event 0x15; any other mode goes straight to
/// the presentation layer. A full table drops the request, as the
/// original does.
#[allow(clippy::too_many_arguments)]
pub fn request(g: &mut GameState, cat: u8, idx: u8, sub: u8, vol: u8, map: usize, x: i32, y: i32, mode: i8) {
    request_keyed(g, cat, idx, sub, vol, map, x, y, mode, 0);
}

/// `request` with the play function's extra byte argument, which becomes a
/// delayed sound's event priority (0x15CA9 stores it at event +5): it
/// orders the event against others due on the same tick and, through the
/// timeline's free list, which record later events receive.
#[allow(clippy::too_many_arguments)]
pub fn request_keyed(g: &mut GameState, cat: u8, idx: u8, sub: u8, vol: u8, map: usize, x: i32, y: i32, mode: i8, key: u8) {
    if mode < 2 {
        g.effects.push(Effect::SoundAt { cat, idx, sub, map, x, y, vol, mode });
        return;
    }
    let Some(slot) = g.delayed_sounds.iter().position(Option::is_none) else { return };
    g.delayed_sounds[slot] = Some(DelayedSound { cat, idx, sub, vol, map, x, y });
    let mut ev = Event::new(EV_DELAYED_SOUND, map as u8, g.tick.wrapping_add(mode as u32 - 1));
    ev.prio = key;
    let [lo, hi] = (slot as u16).to_le_bytes();
    ev.x = lo;
    ev.y = hi;
    g.schedule(ev);
}

/// Event 0x15 (0x160DB): play the slot if the party is still on its map,
/// then free it.
pub fn event(g: &mut GameState, ev: Event) {
    let slot = u16::from_le_bytes([ev.x, ev.y]) as usize;
    let Some(Some(s)) = g.delayed_sounds.get(slot).copied() else { return };
    g.delayed_sounds[slot] = None;
    if s.map == g.party.map {
        g.effects.push(Effect::SoundAt { cat: s.cat, idx: s.idx, sub: s.sub, map: s.map, x: s.x, y: s.y, vol: s.vol, mode: 1 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm2_formats::dungeon::Dungeon;

    fn game() -> Option<GameState> {
        let bytes = std::fs::read(crate::assets::default_data_dir().join("DUNGEON.DAT")).ok()?;
        Some(GameState::new_game(&Dungeon::parse(&bytes).ok()?))
    }

    fn sounds(g: &GameState) -> usize {
        g.effects.iter().filter(|e| matches!(e, Effect::SoundAt { .. })).count()
    }

    #[test]
    fn mode_two_or_more_plays_after_the_delay() {
        let Some(mut g) = game() else { return };
        let p = g.party;
        request(&mut g, 0x17, 2, 0, 0x40, p.map, p.x, p.y, 4);
        assert_eq!(sounds(&g), 0, "held, not played");
        assert!(g.delayed_sounds.iter().any(Option::is_some));
        // Due at tick + 3.
        for _ in 0..3 {
            g.advance();
            assert_eq!(sounds(&g), 0);
        }
        g.advance();
        assert_eq!(sounds(&g), 1);
        assert!(g.delayed_sounds.iter().all(Option::is_none), "slot freed");
    }

    #[test]
    fn delayed_sound_is_dropped_after_leaving_the_map() {
        let Some(mut g) = game() else { return };
        let p = g.party;
        request(&mut g, 0x17, 2, 0, 0x40, p.map, p.x, p.y, 2);
        g.party.map = (p.map + 1) % g.dungeon.maps.len();
        g.advance();
        g.advance();
        assert_eq!(sounds(&g), 0);
        assert!(g.delayed_sounds.iter().all(Option::is_none));
    }

    #[test]
    fn modes_below_two_play_at_once_and_a_full_table_drops() {
        let Some(mut g) = game() else { return };
        let p = g.party;
        request(&mut g, 3, 0, 0x88, 200, p.map, p.x, p.y, 1);
        assert_eq!(sounds(&g), 1);
        for _ in 0..SLOTS + 2 {
            request(&mut g, 3, 0, 0x88, 200, p.map, p.x, p.y, 15);
        }
        assert!(g.delayed_sounds.iter().all(Option::is_some));
        let held = g.timeline.iter().filter(|(_, e)| e.kind == EV_DELAYED_SOUND).count();
        assert_eq!(held, SLOTS, "one event per slot; the extra requests are dropped");
    }
}
