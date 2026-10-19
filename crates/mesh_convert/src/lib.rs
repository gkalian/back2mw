//! Конвертация мешей: NIF 20.0.0.x (Oblivion) → 4.0.0.2 (Morrowind).
//!
//! Что в этом слое нельзя забыть:
//!
//! * **Разбор NIF источника пишем сами.** Это единственный слой, где Rust
//!   слабее Python: готовой библиотеки нет. Крейт `nif` (amPerl) заброшен,
//!   «super-primitive», и годится только как справка.
//! * **Текстуры не конвертируются, а копируются.** Morrowind понимает DDS
//!   Oblivion. Переносить нужно лишь слоты, выжившие после замены шейдерных
//!   свойств, — диффуз и glow; нормали и спекуляр ванильный движок не
//!   использует, и тащить их значит раздувать мод мёртвым весом.
//! * **Коллизия: havok-юниты умножаются на 7.** Подтверждено на исходниках
//!   референса (`asset_convert/collision/collision_extract.py:43`).
//!   `bhk*` разбирается в треугольники внутри `RootCollisionNode`.

/// Игровых юнитов в одном havok-юните у Oblivion.
pub const HAVOK_UNITS_TO_GAME: f32 = 7.0;

pub mod audio;
pub mod collision;
pub mod creature;
pub mod door;
pub mod face_texture;
pub mod facegen;
pub mod geometry;
pub mod head;
pub mod icons;
pub mod material;
pub mod particles;
pub mod read;
pub mod skin;
pub mod speedtree;
pub mod textures;

/// Версия NIF, которую понимает Morrowind.
pub const NIF_VERSION_TES3: u32 = 0x0400_0002;

/// Версия NIF основной массы Oblivion: 90.7% архива и все 4 712 читаемых мешей `STAT`.
pub const NIF_VERSION_TES4: u32 = 0x1400_0004;

/// Редкая версия Oblivion (1.2% архива); в мешах `STAT` не встретилась, раскладка не
/// проверена на данных.
pub const NIF_VERSION_TES4_ALT: u32 = 0x1400_0005;

/// Узел, в котором Morrowind ищет коллизию. Пустой узел движок трактует как
/// «коллизия только для камеры».
pub const ROOT_COLLISION_NODE: &str = "RootCollisionNode";

/// Начало имени узла, занятое Construction Set: узел, чьё имя начинается на `Collision` (девять
/// знаков, без учёта регистра), редактор при загрузке модели переносит в отдельный узел коллизии
/// объекта, который сам вешает под корень меша. Если так назван корень, узел коллизии
/// становится собственным потомком, и редактор падает с переполнением стека, когда грузит модель
/// такого объекта (дамп 2026-10-02: `CollisionBox01` и `CollisionBoxStatic` Oblivion — 363
/// ссылки `SEWorld`; разбор — `docs/ru/formats/tes3.md`, «Имя узла `Collision…`»). Делает ли то
/// же движок игры — не проверено.
pub const RESERVED_NODE_PREFIX: &str = "collision";

/// Что ставится перед именем узла, начинающимся на [`RESERVED_NODE_PREFIX`].
pub const NODE_NAME_GUARD: &str = "ob_";

/// Имя узла выхода по имени узла источника: как есть, кроме имён на [`RESERVED_NODE_PREFIX`] —
/// перед ними ставится [`NODE_NAME_GUARD`]. По именам узлов мировых объектов Morrowind ничего
/// не ищет, так что переименование ничего не ломает; скелеты существ и тел идут мимо — их кости
/// ищутся по имени.
pub fn node_name(name: &str) -> String {
    let reserved = name
        .as_bytes()
        .get(..RESERVED_NODE_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(RESERVED_NODE_PREFIX.as_bytes()));
    if reserved {
        format!("{NODE_NAME_GUARD}{name}")
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::node_name;

    #[test]
    fn a_node_name_never_starts_with_collision() {
        assert_eq!(node_name("CollisionBox01"), "ob_CollisionBox01");
        assert_eq!(node_name("COLLISIONgarlic"), "ob_COLLISIONgarlic");
        assert_eq!(node_name("collision"), "ob_collision");
        // Контроль: остальные имена не трогаются, в том числе узел коллизии Morrowind и имя
        // короче девяти знаков.
        assert_eq!(node_name("RootCollisionNode"), "RootCollisionNode");
        assert_eq!(node_name("Collisio"), "Collisio");
        assert_eq!(node_name("c_col"), "c_col");
        assert_eq!(node_name(""), "");
    }
}
