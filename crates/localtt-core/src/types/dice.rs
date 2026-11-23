//! The dice AST and the persisted roll record.

use std::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::types::{Audience, EntityId, PlayerId, RollId, StatKey, UnixTime};

/// Structured dice AST; part of the serialized [`Roll`] contract. Parser and
/// roller live in the engine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiceExpr {
    /// A constant term.
    Const {
        /// The value.
        value: i32,
    },
    /// A dice pool, e.g. `4d6kh3`.
    Roll {
        /// Number of dice.
        count: i32,
        /// Die sides.
        sides: i32,
        /// Keep rule.
        #[serde(skip_serializing_if = "Option::is_none")]
        keep: Option<Keep>,
    },
    /// Roll twice, keep the better result (usually the d20 term).
    Advantage {
        /// The inner expression.
        inner: Box<DiceExpr>,
    },
    /// Roll twice, keep the worse result.
    Disadvantage {
        /// The inner expression.
        inner: Box<DiceExpr>,
    },
    /// Additional term.
    Add {
        /// Left operand.
        lhs: Box<DiceExpr>,
        /// Right operand.
        rhs: Box<DiceExpr>,
    },
    /// Subtracted term.
    Sub {
        /// Left operand.
        lhs: Box<DiceExpr>,
        /// Right operand.
        rhs: Box<DiceExpr>,
    },
    /// Multiplied term.
    Mul {
        /// Left operand.
        lhs: Box<DiceExpr>,
        /// Right operand.
        rhs: Box<DiceExpr>,
    },
}

/// Keep rule for a dice pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Keep {
    /// Keep the highest dice.
    Highest {
        /// How many dice to keep.
        count: i32,
    },
    /// Keep the lowest dice.
    Lowest {
        /// How many dice to keep.
        count: i32,
    },
}

/// A persisted, replayable roll.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roll {
    /// Roll id.
    pub id: RollId,
    /// Who rolled.
    pub roller: PlayerId,
    /// The entity the roll was made for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<EntityId>,
    /// Wall-clock time.
    pub ts: UnixTime,
    /// The composed expression that was actually rolled.
    pub expr: DiceExpr,
    /// The result breakdown.
    pub result: DiceResult,
    /// What the roll was for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<RollContext>,
    /// Who sees the roll.
    pub audience: Audience,
}

/// Per-term and per-die breakdown, so frontends can render dice, detect nat 1
/// and nat 20 under advantage, and attribute composed terms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiceResult {
    /// The terms of the expression.
    pub terms: Vec<TermResult>,
    /// The summed total.
    pub total: i32,
}

/// Result of one term.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermResult {
    /// The dice rolled for this term.
    pub dice: Vec<DieResult>,
    /// The summed term total.
    pub total: i32,
    /// Attribution label for a composed term, e.g. `"Bless"`; `None` for the
    /// base expression. A label snapshot, not an id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Result of one die.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DieResult {
    /// The rolled value.
    pub value: i32,
    /// Whether the die was kept by its keep rule.
    pub kept: bool,
}

/// What a roll was for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollContext {
    /// The roll category.
    pub kind: RollKind,
    /// The composed stat, e.g. `"skill.stealth"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stat: Option<StatKey>,
    /// The DC the roll was against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_dc: Option<i32>,
}

/// Roll category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollKind {
    /// Ability or skill check.
    Check,
    /// Saving throw.
    Save,
    /// Attack roll.
    Attack,
    /// Damage roll.
    Damage,
    /// Anything else.
    Other,
}
