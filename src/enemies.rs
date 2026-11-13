//! Enemy type table, recovered from the original PowerPC executable (see tools/re/enemy_types.py and RE_NOTES.md).
//! Generated from data/enemy_types.json; do not edit by hand.
//!
//! A spawn group (`Level::groups[i].u32[11]`) names an enemy type; `u32[12]` (<= 10) is how many enemies it launches.
//! Fields marked "constants" are what each type's init routine stores into a new enemy slot. Behaviour (movement, firing)
//! lives in the routine at `behaviour_code` (a code-section offset) and is NOT yet ported.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyType {
    pub id: u32,
    /// Offset of the type's behaviour routine in the original executable's code section.
    pub behaviour_code: u32,
    /// 1-based entry in the sprite animation table (sprites/sets.json, set 1000).
    pub anim_index: u32,
    /// Sprite size in pixels (matches the animation's first frame for 20 of 22 types; types 1 and 2 differ).
    pub sprite_w: i32,
    pub sprite_h: i32,
    /// Roughly half the sprite size; used by the original for hit testing.
    pub hit_w: i32,
    pub hit_h: i32,
    /// Stored at enemy+84; probably hit points (values 1, 3, 4, 8).
    pub field_84: i32,
    /// Points awarded (stored at enemy+132).
    pub score: i32,
}

pub const ENEMY_TYPES: [EnemyType; 22] = [
    EnemyType { id: 1, behaviour_code: 0x33cf4, anim_index: 142, sprite_w: 32, sprite_h: 32, hit_w: 16, hit_h: 16, field_84: 1, score: 30 },
    EnemyType { id: 2, behaviour_code: 0x344e8, anim_index: 142, sprite_w: 32, sprite_h: 32, hit_w: 16, hit_h: 16, field_84: 1, score: 50 },
    EnemyType { id: 3, behaviour_code: 0x33a64, anim_index: 141, sprite_w: 56, sprite_h: 56, hit_w: 28, hit_h: 28, field_84: 1, score: 1 },
    EnemyType { id: 4, behaviour_code: 0x34868, anim_index: 148, sprite_w: 16, sprite_h: 16, hit_w: 8, hit_h: 8, field_84: 4, score: 100 },
    EnemyType { id: 5, behaviour_code: 0x340d0, anim_index: 142, sprite_w: 40, sprite_h: 40, hit_w: 20, hit_h: 20, field_84: 1, score: 50 },
    EnemyType { id: 6, behaviour_code: 0x34b28, anim_index: 150, sprite_w: 26, sprite_h: 26, hit_w: 13, hit_h: 13, field_84: 1, score: 100 },
    EnemyType { id: 7, behaviour_code: 0x34de0, anim_index: 151, sprite_w: 37, sprite_h: 61, hit_w: 18, hit_h: 30, field_84: 1, score: 500 },
    EnemyType { id: 8, behaviour_code: 0x35124, anim_index: 128, sprite_w: 45, sprite_h: 35, hit_w: 22, hit_h: 17, field_84: 1, score: 100 },
    EnemyType { id: 9, behaviour_code: 0x35410, anim_index: 129, sprite_w: 45, sprite_h: 35, hit_w: 22, hit_h: 17, field_84: 8, score: 100 },
    EnemyType { id: 10, behaviour_code: 0x35714, anim_index: 119, sprite_w: 58, sprite_h: 62, hit_w: 29, hit_h: 31, field_84: 1, score: 400 },
    EnemyType { id: 11, behaviour_code: 0x359ec, anim_index: 150, sprite_w: 26, sprite_h: 26, hit_w: 13, hit_h: 13, field_84: 1, score: 50 },
    EnemyType { id: 12, behaviour_code: 0x35d38, anim_index: 150, sprite_w: 26, sprite_h: 26, hit_w: 13, hit_h: 13, field_84: 3, score: 50 },
    EnemyType { id: 13, behaviour_code: 0x36080, anim_index: 150, sprite_w: 26, sprite_h: 26, hit_w: 13, hit_h: 13, field_84: 1, score: 50 },
    EnemyType { id: 14, behaviour_code: 0x363b4, anim_index: 128, sprite_w: 45, sprite_h: 35, hit_w: 22, hit_h: 17, field_84: 1, score: 50 },
    EnemyType { id: 15, behaviour_code: 0x36724, anim_index: 138, sprite_w: 45, sprite_h: 48, hit_w: 22, hit_h: 24, field_84: 1, score: 200 },
    EnemyType { id: 16, behaviour_code: 0x369ec, anim_index: 133, sprite_w: 32, sprite_h: 32, hit_w: 16, hit_h: 16, field_84: 1, score: 300 },
    EnemyType { id: 17, behaviour_code: 0x36c88, anim_index: 143, sprite_w: 32, sprite_h: 32, hit_w: 16, hit_h: 16, field_84: 1, score: 100 },
    EnemyType { id: 18, behaviour_code: 0x36fd4, anim_index: 144, sprite_w: 32, sprite_h: 32, hit_w: 16, hit_h: 16, field_84: 1, score: 300 },
    EnemyType { id: 19, behaviour_code: 0x3727c, anim_index: 130, sprite_w: 64, sprite_h: 64, hit_w: 32, hit_h: 32, field_84: 1, score: 400 },
    EnemyType { id: 20, behaviour_code: 0x37568, anim_index: 153, sprite_w: 64, sprite_h: 64, hit_w: 32, hit_h: 32, field_84: 1, score: 400 },
    EnemyType { id: 21, behaviour_code: 0x3785c, anim_index: 142, sprite_w: 40, sprite_h: 40, hit_w: 20, hit_h: 20, field_84: 1, score: 50 },
    EnemyType { id: 23, behaviour_code: 0x37c74, anim_index: 157, sprite_w: 58, sprite_h: 58, hit_w: 29, hit_h: 29, field_84: 1, score: 500 },
];

pub fn enemy_type(id: u32) -> Option<&'static EnemyType> {
    ENEMY_TYPES.iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_well_formed() {
        assert_eq!(ENEMY_TYPES.len(), 22);
        let mut ids: Vec<u32> = ENEMY_TYPES.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 22, "type ids must be unique");
        assert!(ENEMY_TYPES.iter().all(|t| (119..=159).contains(&t.anim_index)));
        assert_eq!(enemy_type(7).unwrap().score, 500);
        assert!(enemy_type(22).is_none()); // type 22 does not exist (the original's table skips it)
    }
}
