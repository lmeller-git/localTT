//! Modifiers, the formula language and modifier conditions.

use serde::{Deserialize, Serialize};

use crate::types::{Ability, DiceExpr, StatKey};

/// A modifier contributed by an item, feature or status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifier {
    /// The stat (or group) the modifier applies to.
    pub target: StatKey,
    /// The operation, carrying its operand.
    #[serde(flatten)]
    pub op: ModifierOp,
    /// Condition that must hold for the modifier to apply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<Condition>,
    /// Human-readable attribution, e.g. `"Bless"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The operation carries its operand, so "advantage with a value" is not
/// representable; flattened into [`Modifier`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ModifierOp {
    /// Add the value.
    Add {
        /// The addend.
        value: Formula,
    },
    /// Set the value.
    Set {
        /// The value to set.
        value: Formula,
    },
    /// Clamp the value to at least this.
    Min {
        /// The lower bound.
        value: Formula,
    },
    /// Clamp the value to at most this.
    Max {
        /// The upper bound.
        value: Formula,
    },
    /// Grant advantage.
    Advantage,
    /// Grant disadvantage.
    Disadvantage,
}

/// Small closed expression language, not a scripting language.
///
/// Untagged so the common case is a bare number: `2` or
/// `{"type":"proficiency_bonus"}`. Dice leaves only evaluate in roll
/// contexts; static contexts use the flat part.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Formula {
    /// Constant shorthand.
    Const(i32),
    /// A tagged expression.
    Expr(FormulaExpr),
}

/// Tagged formula expressions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FormulaExpr {
    /// The proficiency bonus.
    ProficiencyBonus,
    /// The modifier of an ability.
    AbilityMod {
        /// The ability.
        ability: Ability,
    },
    /// Total character level.
    Level,
    /// Level in a class, matched by name snapshot.
    ClassLevel {
        /// The class name.
        class: String,
    },
    /// Addition.
    Add {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Subtraction.
    Sub {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Multiplication.
    Mul {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Minimum.
    Min {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Maximum.
    Max {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Floor division.
    FloorDiv {
        /// Left operand.
        lhs: Box<Formula>,
        /// Right operand.
        rhs: Box<Formula>,
    },
    /// Dice term, e.g. Bless's `1d4`.
    Dice {
        /// The dice expression.
        expr: DiceExpr,
    },
}

/// Closed condition set; engine matching is case-insensitive for names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Condition {
    /// The item is equipped.
    Equipped,
    /// The item is attuned.
    Attuned,
    /// A status with this name snapshot is active.
    StatusActive {
        /// The status name.
        name: String,
    },
    /// A table-provided condition matched by name, e.g. "while raging"
    /// (RFC-0001 5.10): core stores the name, the engine decides the effects.
    Custom {
        /// The condition name.
        name: String,
    },
}
