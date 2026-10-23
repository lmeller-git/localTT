//! Pure command builders for the browser forms.
//!
//! These types mirror the small TUI forms but build transport DTOs directly.
//! Keeping them native makes validation and wire-shape tests run without a
//! browser.

use localtt_core::{
    builders::StatblockBuilder,
    types::{
        Abilities,
        AnyEntity,
        Entity,
        EntityId,
        EntityKind,
        Ext,
        HandoutBody,
        HandoutContent,
        InlineItem,
        InventoryEntryId,
        InventoryItem,
        ItemKind,
        ItemState,
        PlayerId,
        RefOrText,
        RulesetId,
    },
};
use localtt_protocol::{CommandDto, RollSpecDto, parse_dice};

/// The small character-creation form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterDraft {
    /// Character name.
    pub name: String,
    /// Class name.
    pub class: String,
    /// Level as entered.
    pub level: String,
    /// Maximum (and starting) hit points as entered.
    pub max_hp: String,
    /// Six ability scores, or `standard`.
    pub abilities: String,
}

impl Default for CharacterDraft {
    #[inline]
    fn default() -> Self {
        Self {
            name: String::new(),
            class: "Fighter".to_owned(),
            level: "1".to_owned(),
            max_hp: "10".to_owned(),
            abilities: "standard".to_owned(),
        }
    }
}

impl CharacterDraft {
    /// Builds a player-character creation command.
    ///
    /// # Errors
    ///
    /// A display message when a field is empty, out of range, or the core
    /// builder rejects the document.
    pub fn command(&self, actor: PlayerId, ruleset: &RulesetId) -> Result<CommandDto, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("name is required".to_owned());
        }
        if name.chars().count() > 120 {
            return Err("name must be at most 120 characters".to_owned());
        }
        let class = self.class.trim();
        if class.is_empty() {
            return Err("class is required".to_owned());
        }
        if class.chars().count() > 80 {
            return Err("class must be at most 80 characters".to_owned());
        }
        let level = parse_number(&self.level, "level", 1, 20)?;
        let hp = parse_number(&self.max_hp, "max hp", 1, 100_000)?;
        let abilities = parse_abilities(&self.abilities)?;

        let mut builder = StatblockBuilder::character();
        builder.abilities(abilities).hp(hp, hp);
        builder.class_level(RefOrText::Text(class.to_owned()), level, None);
        let mut entity = builder
            .build_entity(EntityKind::Pc, EntityId::new(), name, actor)
            .map_err(|error| format!("could not build character: {error}"))?;
        entity.ruleset = ruleset.clone();
        Ok(CommandDto::CreateEntity {
            entity: Box::new(AnyEntity::from(entity)),
        })
    }
}

/// Whether a handout carries text or an item definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoutKind {
    /// Free text.
    Text,
    /// An item that can be given to a statblock.
    Item,
}

impl HandoutKind {
    /// The value used by the form select.
    #[must_use]
    #[inline]
    pub const fn value(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Item => "item",
        }
    }

    /// Reads a form value.
    #[must_use]
    #[inline]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "text" => Some(Self::Text),
            "item" => Some(Self::Item),
            _ => None,
        }
    }
}

/// The small handout-creation form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandoutDraft {
    /// Text or item.
    pub kind: HandoutKind,
    /// Document name.
    pub name: String,
    /// Text body or item name.
    pub content: String,
}

impl Default for HandoutDraft {
    #[inline]
    fn default() -> Self {
        Self {
            kind: HandoutKind::Text,
            name: String::new(),
            content: String::new(),
        }
    }
}

impl HandoutDraft {
    /// Builds a handout creation command.
    ///
    /// # Errors
    ///
    /// A display message when a field is empty or the core builder rejects the
    /// document.
    pub fn command(&self, actor: PlayerId, ruleset: &RulesetId) -> Result<CommandDto, String> {
        let name = self.name.trim();
        let content = self.content.trim();
        if name.is_empty() {
            return Err("the handout name must not be empty".to_owned());
        }
        if name.chars().count() > 120 {
            return Err("the handout name must be at most 120 characters".to_owned());
        }
        if content.is_empty() {
            return Err("the handout content must not be empty".to_owned());
        }
        let body = HandoutBody {
            content: match self.kind {
                HandoutKind::Text => HandoutContent::Text {
                    text: content.to_owned(),
                },
                HandoutKind::Item => HandoutContent::Item {
                    item: inline_item(content),
                },
            },
        };
        let mut entity = Entity::builder(EntityKind::Handout, EntityId::new(), name, actor, body)
            .build()
            .map_err(|error| format!("could not build handout: {error}"))?;
        entity.ruleset = ruleset.clone();
        Ok(CommandDto::CreateEntity {
            entity: Box::new(AnyEntity::from(entity)),
        })
    }
}

/// The inventory add form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemDraft {
    /// Item name.
    pub name: String,
    /// Quantity as entered.
    pub qty: String,
    /// Carried, equipped or stowed.
    pub state: ItemState,
}

impl Default for ItemDraft {
    #[inline]
    fn default() -> Self {
        Self {
            name: String::new(),
            qty: "1".to_owned(),
            state: ItemState::Carried,
        }
    }
}

impl ItemDraft {
    /// Builds an add-inventory command for `entity`.
    ///
    /// # Errors
    ///
    /// A display message when the name is empty or the quantity is invalid.
    pub fn add_command(&self, entity: EntityId) -> Result<CommandDto, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("item name is required".to_owned());
        }
        if name.chars().count() > 120 {
            return Err("item name must be at most 120 characters".to_owned());
        }
        let qty = parse_quantity(&self.qty)?;
        Ok(CommandDto::AddInventoryItem {
            entity,
            item: Box::new(InventoryItem::Inline(inline_item(name))),
            qty,
            state: self.state,
            attuned: false,
            notes: String::new(),
            expected_rev: None,
        })
    }
}

/// Builds an inventory edit command.
#[must_use]
#[inline]
pub fn edit_item_command(
    entity: EntityId,
    entry: InventoryEntryId,
    qty: i32,
    state: ItemState,
    attuned: bool,
    notes: String,
) -> CommandDto {
    CommandDto::EditInventoryItem {
        entity,
        entry,
        qty: Some(qty),
        state: Some(state),
        attuned: Some(attuned),
        notes: Some(notes),
        expected_rev: None,
    }
}

/// Builds an inventory removal command.
#[must_use]
#[inline]
pub fn remove_item_command(entity: EntityId, entry: InventoryEntryId) -> CommandDto {
    CommandDto::RemoveInventoryItem {
        entity,
        entry,
        expected_rev: None,
    }
}

/// The units offered by the clock form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockUnit {
    /// Seconds.
    Seconds,
    /// Minutes.
    Minutes,
    /// Hours.
    Hours,
    /// Days.
    Days,
}

impl ClockUnit {
    /// The value used by the form select.
    #[must_use]
    #[inline]
    pub const fn value(self) -> &'static str {
        match self {
            Self::Seconds => "seconds",
            Self::Minutes => "minutes",
            Self::Hours => "hours",
            Self::Days => "days",
        }
    }

    /// Reads a form value.
    #[must_use]
    #[inline]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "seconds" => Some(Self::Seconds),
            "minutes" => Some(Self::Minutes),
            "hours" => Some(Self::Hours),
            "days" => Some(Self::Days),
            _ => None,
        }
    }

    /// Seconds in one unit.
    #[must_use]
    #[inline]
    pub const fn seconds(self) -> i64 {
        match self {
            Self::Seconds => 1,
            Self::Minutes => 60,
            Self::Hours => 3_600,
            Self::Days => 86_400,
        }
    }
}

/// Builds a clock-advance command.
///
/// # Errors
///
/// A display message when the amount is not a whole number, is zero, or the
/// converted value overflows.
#[inline]
pub fn advance_clock_command(amount: &str, unit: ClockUnit) -> Result<CommandDto, String> {
    let amount = amount
        .trim()
        .parse::<i64>()
        .map_err(|_| "clock advance must be a whole number".to_owned())?;
    let seconds = amount
        .checked_mul(unit.seconds())
        .ok_or_else(|| "clock advance is too large".to_owned())?;
    if seconds == 0 {
        return Err("clock advance must not be zero".to_owned());
    }
    Ok(CommandDto::AdvanceClock {
        by_seconds: seconds,
    })
}

/// Builds a dice-roll command.
///
/// # Errors
///
/// A display message when the expression is empty or invalid.
#[inline]
pub fn roll_command(expression: &str, entity: Option<EntityId>) -> Result<CommandDto, String> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err("roll expression is required".to_owned());
    }
    let expr = parse_dice(expression)?;
    Ok(CommandDto::Roll {
        entity,
        spec: RollSpecDto::Expression { expr, target: None },
        audience: None,
    })
}

/// Parses an inventory quantity.
///
/// # Errors
///
/// A display message when the value is not a whole number or is out of range.
#[inline]
pub fn parse_quantity(raw: &str) -> Result<i32, String> {
    let value = raw
        .trim()
        .parse::<i32>()
        .map_err(|_| "quantity must be a whole number".to_owned())?;
    if !(0..=1_000_000).contains(&value) {
        return Err("quantity must be between 0 and 1000000".to_owned());
    }
    Ok(value)
}

/// Parses a non-negative command amount.
///
/// # Errors
///
/// A display message when the value is not a whole number or is out of range.
#[inline]
pub fn parse_amount(raw: &str) -> Result<i32, String> {
    let value = raw
        .trim()
        .parse::<i32>()
        .map_err(|_| "amount must be a whole number".to_owned())?;
    if !(0..=1_000_000).contains(&value) {
        return Err("amount must be between 0 and 1000000".to_owned());
    }
    Ok(value)
}

/// Reads an inventory-state form value.
#[must_use]
#[inline]
pub fn parse_item_state(value: &str) -> Option<ItemState> {
    match value {
        "carried" => Some(ItemState::Carried),
        "equipped" => Some(ItemState::Equipped),
        "stowed" => Some(ItemState::Stowed),
        _ => None,
    }
}

fn inline_item(name: &str) -> InlineItem {
    InlineItem {
        name: name.to_owned(),
        description: String::new(),
        kind: ItemKind::Gear,
        requires_attunement: false,
        weight_oz: None,
        value: None,
        properties: None,
        modifiers: Vec::new(),
        resource: None,
        ext: Ext::new(),
    }
}

fn parse_number(raw: &str, label: &str, min: i32, max: i32) -> Result<i32, String> {
    let value = raw
        .trim()
        .parse::<i32>()
        .map_err(|_| format!("{label} must be a whole number"))?;
    if !(min..=max).contains(&value) {
        return Err(format!("{label} must be between {min} and {max}"));
    }
    Ok(value)
}

fn parse_abilities(raw: &str) -> Result<Abilities, String> {
    let values = if raw.trim().eq_ignore_ascii_case("standard") {
        vec![15, 14, 13, 12, 10, 8]
    } else {
        raw.split(|character: char| character == ',' || character.is_whitespace())
            .filter(|part| !part.is_empty())
            .map(|part| {
                part.parse::<i32>()
                    .map_err(|_| "abilities must be six whole numbers".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    if values.len() != 6 {
        return Err("abilities must be six scores, for example 15,14,13,12,10,8".to_owned());
    }
    if values.iter().any(|value| !(1..=30).contains(value)) {
        return Err("ability scores must be between 1 and 30".to_owned());
    }
    Ok(Abilities {
        strength: values[0],
        dexterity: values[1],
        constitution: values[2],
        intelligence: values[3],
        wisdom: values[4],
        charisma: values[5],
    })
}

#[cfg(test)]
mod tests {
    use localtt_core::types::RulesetId;

    use super::*;

    fn ids() -> (PlayerId, RulesetId) {
        (
            PlayerId::new(),
            RulesetId::parse("5e-2014").expect("a valid ruleset"),
        )
    }

    #[test]
    fn a_character_draft_builds_a_player_character() {
        let (actor, ruleset) = ids();
        let draft = CharacterDraft {
            name: "Aria".to_owned(),
            class: "Ranger".to_owned(),
            level: "3".to_owned(),
            max_hp: "24".to_owned(),
            abilities: "12,14,13,10,15,8".to_owned(),
        };

        let CommandDto::CreateEntity { entity } =
            draft.command(actor, &ruleset).expect("the draft is valid")
        else {
            panic!("a character is a create command");
        };
        assert_eq!(entity.kind(), EntityKind::Pc);
        assert_eq!(entity.name(), "Aria");
        assert_eq!(entity.owner(), actor);
        assert_eq!(entity.ruleset(), &ruleset);
        assert_eq!(entity.rev(), 0);
        let AnyEntity::Statblock(block) = entity.as_ref() else {
            panic!("a character has a statblock body");
        };
        assert_eq!(block.body.hp.max, 24);
        assert_eq!(block.body.abilities.strength, 12);
        assert_eq!(block.body.levels[0].level, 3);
        assert_eq!(
            block.body.levels[0].class,
            RefOrText::Text("Ranger".to_owned())
        );
    }

    #[test]
    fn a_character_draft_rejects_bad_fields() {
        let (actor, ruleset) = ids();
        let mut draft = CharacterDraft::default();
        assert_eq!(
            draft.command(actor, &ruleset),
            Err("name is required".to_owned())
        );

        draft.name = "Aria".to_owned();
        draft.level = "21".to_owned();
        assert_eq!(
            draft.command(actor, &ruleset),
            Err("level must be between 1 and 20".to_owned())
        );

        draft.level = "1".to_owned();
        draft.abilities = "1,2,3".to_owned();
        assert_eq!(
            draft.command(actor, &ruleset),
            Err("abilities must be six scores, for example 15,14,13,12,10,8".to_owned())
        );
    }

    #[test]
    fn a_handout_carries_text_or_an_item() {
        let (actor, ruleset) = ids();
        let text = HandoutDraft {
            kind: HandoutKind::Text,
            name: "A note".to_owned(),
            content: "Meet at dusk.".to_owned(),
        };
        let CommandDto::CreateEntity { entity } =
            text.command(actor, &ruleset).expect("text builds")
        else {
            panic!("a handout is a create command");
        };
        assert_eq!(entity.kind(), EntityKind::Handout);
        let AnyEntity::Handout(handout) = entity.as_ref() else {
            panic!("a handout has a handout body");
        };
        assert_eq!(
            handout.body.content,
            HandoutContent::Text {
                text: "Meet at dusk.".to_owned()
            }
        );

        let item = HandoutDraft {
            kind: HandoutKind::Item,
            name: "Potion".to_owned(),
            content: "Potion of Healing".to_owned(),
        };
        let CommandDto::CreateEntity { entity } =
            item.command(actor, &ruleset).expect("item builds")
        else {
            panic!("a handout is a create command");
        };
        let AnyEntity::Handout(handout) = entity.as_ref() else {
            panic!("a handout has a handout body");
        };
        let HandoutContent::Item { item } = &handout.body.content else {
            panic!("the item handout carries an item");
        };
        assert_eq!(item.name, "Potion of Healing");
        assert_eq!(item.kind, ItemKind::Gear);
    }

    #[test]
    fn items_validate_quantity_and_carry_a_state() {
        let entity = EntityId::new();
        let draft = ItemDraft {
            name: "Rope".to_owned(),
            qty: "2".to_owned(),
            state: ItemState::Equipped,
        };
        let CommandDto::AddInventoryItem {
            entity: target,
            item,
            qty,
            state,
            ..
        } = draft.add_command(entity).expect("the item builds")
        else {
            panic!("an item add command");
        };
        assert_eq!(target, entity);
        assert_eq!(qty, 2);
        assert_eq!(state, ItemState::Equipped);
        assert_eq!(crate::format::item_name(&item), "Rope");

        assert_eq!(
            ItemDraft {
                name: "Rope".to_owned(),
                qty: "many".to_owned(),
                state: ItemState::Carried,
            }
            .add_command(entity),
            Err("quantity must be a whole number".to_owned())
        );
        assert_eq!(
            ItemDraft {
                name: "Rope".to_owned(),
                qty: "1000001".to_owned(),
                state: ItemState::Carried,
            }
            .add_command(entity),
            Err("quantity must be between 0 and 1000000".to_owned())
        );
    }

    #[test]
    fn rolls_use_the_engine_grammar() {
        let entity = EntityId::new();
        let CommandDto::Roll {
            entity: target,
            spec,
            ..
        } = roll_command("1d20+3", Some(entity)).expect("the roll parses")
        else {
            panic!("a roll command");
        };
        assert_eq!(target, Some(entity));
        assert!(matches!(spec, RollSpecDto::Expression { .. }));

        assert_eq!(
            roll_command("", None),
            Err("roll expression is required".to_owned())
        );
        assert!(roll_command("not dice", None).is_err());
    }

    #[test]
    fn clock_units_convert_and_overflow_is_rejected() {
        assert_eq!(
            advance_clock_command("2", ClockUnit::Hours),
            Ok(CommandDto::AdvanceClock { by_seconds: 7_200 })
        );
        assert_eq!(
            advance_clock_command("0", ClockUnit::Minutes),
            Err("clock advance must not be zero".to_owned())
        );
        assert_eq!(
            advance_clock_command("9223372036854775807", ClockUnit::Days),
            Err("clock advance is too large".to_owned())
        );
    }

    #[test]
    fn item_and_handout_values_round_trip() {
        assert_eq!(HandoutKind::parse("item"), Some(HandoutKind::Item));
        assert_eq!(HandoutKind::Item.value(), "item");
        assert_eq!(HandoutKind::parse("nope"), None);
        assert_eq!(ClockUnit::parse("days"), Some(ClockUnit::Days));
        assert_eq!(ClockUnit::Days.seconds(), 86_400);
        assert_eq!(ClockUnit::parse("nope"), None);
        assert_eq!(parse_item_state("equipped"), Some(ItemState::Equipped));
        assert_eq!(parse_item_state("nope"), None);
        assert_eq!(parse_amount("12"), Ok(12));
        assert_eq!(
            parse_amount("1000001"),
            Err("amount must be between 0 and 1000000".to_owned())
        );
    }

    #[test]
    fn edit_and_remove_commands_keep_the_entry_id() {
        let entity = EntityId::new();
        let entry = InventoryEntryId::new();
        assert!(matches!(
            edit_item_command(entity, entry, 2, ItemState::Stowed, true, "bag".to_owned()),
            CommandDto::EditInventoryItem { entry: actual, qty: Some(2), .. } if actual == entry
        ));
        assert!(matches!(
            remove_item_command(entity, entry),
            CommandDto::RemoveInventoryItem { entry: actual, .. } if actual == entry
        ));
    }
}
