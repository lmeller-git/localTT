//! Conversions between the engine's `sheet` types and their DTOs.

use localtt_engine::{
    rules::{
        Contribution,
        ContributionOp,
        ContributionSource,
        DerivedAttack,
        DerivedResource,
        DerivedSheet,
        DerivedValue,
        PactSlots,
        RollHint,
        SlotMaxima,
        WeaponAttackKind,
    },
    services::SheetView,
};

use crate::{
    ContributionDto,
    ContributionOpDto,
    ContributionSourceDto,
    DerivedAttackDto,
    DerivedResourceDto,
    DerivedSheetDto,
    DerivedValueDto,
    PactSlotsDto,
    RollHintDto,
    SheetViewDto,
    SlotMaximaDto,
    WarningDto,
    WeaponAttackKindDto,
};

impl From<SheetView> for SheetViewDto {
    #[inline]
    fn from(view: SheetView) -> Self {
        Self {
            entity: view.entity,
            rev: view.rev,
            sheet: view.sheet.into(),
        }
    }
}

impl From<DerivedSheet> for DerivedSheetDto {
    #[inline]
    fn from(sheet: DerivedSheet) -> Self {
        Self {
            values: sheet
                .values
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
            resources: sheet
                .resources
                .into_iter()
                .map(|(id, resource)| (id, resource.into()))
                .collect(),
            slots: sheet.slots.into(),
            hit_dice: sheet.hit_dice,
            warnings: sheet.warnings.into_iter().map(WarningDto::from).collect(),
            attacks: sheet.attacks.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<DerivedValue> for DerivedValueDto {
    #[inline]
    fn from(value: DerivedValue) -> Self {
        Self {
            computed: value.computed,
            override_value: value.override_value,
            effective: value.effective,
            breakdown: value.breakdown.into_iter().map(Into::into).collect(),
            hint: value.hint.map(RollHintDto::from),
        }
    }
}

impl From<Contribution> for ContributionDto {
    #[inline]
    fn from(contribution: Contribution) -> Self {
        Self {
            source: contribution.source.into(),
            label: contribution.label,
            op: contribution.op.into(),
            value: contribution.value,
        }
    }
}

impl From<ContributionSource> for ContributionSourceDto {
    #[inline]
    fn from(source: ContributionSource) -> Self {
        match source {
            ContributionSource::Base => Self::Base,
            ContributionSource::Item(id) => Self::Item { id },
            ContributionSource::Feature(id) => Self::Feature { id },
            ContributionSource::Status(id) => Self::Status { id },
        }
    }
}

impl From<ContributionOp> for ContributionOpDto {
    #[inline]
    fn from(op: ContributionOp) -> Self {
        match op {
            ContributionOp::Set => Self::Set,
            ContributionOp::Add => Self::Add,
            ContributionOp::Min => Self::Min,
            ContributionOp::Max => Self::Max,
            ContributionOp::Advantage => Self::Advantage,
            ContributionOp::Disadvantage => Self::Disadvantage,
        }
    }
}

impl From<DerivedResource> for DerivedResourceDto {
    #[inline]
    fn from(resource: DerivedResource) -> Self {
        Self {
            max: resource.max,
            breakdown: resource.breakdown.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<SlotMaxima> for SlotMaximaDto {
    #[inline]
    fn from(slots: SlotMaxima) -> Self {
        Self {
            spell: slots.spell,
            pact: slots.pact.map(PactSlotsDto::from),
        }
    }
}

impl From<PactSlots> for PactSlotsDto {
    #[inline]
    fn from(slots: PactSlots) -> Self {
        Self {
            level: slots.level,
            count: slots.count,
        }
    }
}

impl From<RollHint> for RollHintDto {
    #[inline]
    fn from(hint: RollHint) -> Self {
        match hint {
            RollHint::Normal => Self::Normal,
            RollHint::Advantage => Self::Advantage,
            RollHint::Disadvantage => Self::Disadvantage,
        }
    }
}

impl From<WeaponAttackKind> for WeaponAttackKindDto {
    #[inline]
    fn from(kind: WeaponAttackKind) -> Self {
        match kind {
            WeaponAttackKind::Melee => Self::Melee,
            WeaponAttackKind::Ranged => Self::Ranged,
        }
    }
}

impl From<DerivedAttack> for DerivedAttackDto {
    #[inline]
    fn from(attack: DerivedAttack) -> Self {
        Self {
            entry: attack.entry,
            name: attack.name,
            kind: attack.kind.into(),
            ability: attack.ability,
            attack_bonus: attack.attack_bonus,
            damage: attack.damage,
            damage_bonus: attack.damage_bonus,
            versatile_damage: attack.versatile_damage,
            damage_type: attack.damage_type,
            range_ft: attack.range_ft,
            hint: attack.hint.into(),
            breakdown: attack.breakdown.into_iter().map(Into::into).collect(),
        }
    }
}
