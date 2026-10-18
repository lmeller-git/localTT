//! One-hop alias resolution (`docs/rfcs/content.md` section 5.4).
//!
//! An alias is an old `RefId` that resolves to one live id of the same
//! snapshot. Alias names are not indexed: `get` is the only alias-aware call.

use std::collections::{BTreeMap, BTreeSet};

use localtt_core::types::{Namespace, RefId};
use localtt_engine::content::ContentFault;

/// A validated `alias -> target` table.
pub(crate) struct AliasTable {
    /// Alias id to live target id; built once and then read-only.
    rows: BTreeMap<RefId, RefId>,
}

impl AliasTable {
    /// Validates and stores `(alias, target)` rows against the live ids and the
    /// namespace of one snapshot.
    ///
    /// Refused as [`ContentFault::Corrupt`]: an alias outside `namespace`, an alias
    /// that equals a live id, a duplicate alias, a target that is not live, and a
    /// target that is itself an alias (chains).
    pub(crate) fn new(
        rows: Vec<(RefId, RefId)>,
        live: &BTreeSet<RefId>,
        namespace: &Namespace,
    ) -> Result<Self, ContentFault> {
        let mut table = BTreeMap::new();
        for (alias, target) in rows {
            if alias.namespace() != namespace.as_str() {
                return Err(ContentFault::Corrupt {
                    message: format!("alias {alias} is outside the snapshot namespace {namespace}"),
                });
            }
            if live.contains(&alias) {
                return Err(ContentFault::Corrupt {
                    message: format!("alias {alias} collides with a live id"),
                });
            }
            if !live.contains(&target) {
                return Err(ContentFault::Corrupt {
                    message: format!("alias {alias} targets {target}, which is not live"),
                });
            }
            // Chains cannot occur: the target is live and live ids are never alias
            // keys, so the target cannot resolve a second time.
            if table.contains_key(&alias) {
                return Err(ContentFault::Corrupt {
                    message: format!("alias {alias} appears twice"),
                });
            }
            table.insert(alias, target);
        }
        Ok(Self { rows: table })
    }

    /// Resolves one id; `Some(target)` when `id` is an alias, `None` otherwise.
    #[inline]
    pub(crate) fn resolve(&self, id: &RefId) -> Option<&RefId> {
        self.rows.get(id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use localtt_core::types::{Namespace, RefId};
    use localtt_engine::content::ContentFault;

    use super::AliasTable;

    fn ref_id(raw: &str) -> RefId {
        RefId::parse(raw).expect("a literal reference")
    }

    fn namespace(raw: &str) -> Namespace {
        Namespace::parse(raw).expect("a literal namespace")
    }

    fn live(ids: &[&str]) -> BTreeSet<RefId> {
        ids.iter().copied().map(ref_id).collect()
    }

    fn table(rows: &[(&str, &str)], live: &BTreeSet<RefId>) -> Result<AliasTable, ContentFault> {
        AliasTable::new(
            rows.iter()
                .map(|(alias, target)| (ref_id(alias), ref_id(target)))
                .collect(),
            live,
            &namespace("pack.test"),
        )
    }

    #[test]
    fn a_valid_table_resolves_only_its_aliases() {
        let live = live(&["pack.test:item/crown-of-dusk"]);
        let table = table(
            &[(
                "pack.test:item/crown-of-dusk-old",
                "pack.test:item/crown-of-dusk",
            )],
            &live,
        )
        .expect("a valid table");

        assert_eq!(
            table
                .resolve(&ref_id("pack.test:item/crown-of-dusk-old"))
                .map(RefId::as_str),
            Some("pack.test:item/crown-of-dusk")
        );
        assert_eq!(table.resolve(&ref_id("pack.test:item/crown-of-dusk")), None);
    }

    #[test]
    fn a_foreign_namespace_alias_is_refused() {
        let live = live(&["pack.test:item/crown-of-dusk"]);

        assert!(matches!(
            table(
                &[(
                    "pack.other:item/crown-of-dusk-old",
                    "pack.test:item/crown-of-dusk"
                )],
                &live
            ),
            Err(ContentFault::Corrupt { .. })
        ));
    }

    #[test]
    fn an_alias_colliding_with_a_live_id_is_refused() {
        let live = live(&["pack.test:item/crown-of-dusk"]);

        assert!(matches!(
            table(
                &[(
                    "pack.test:item/crown-of-dusk",
                    "pack.test:item/crown-of-dusk"
                )],
                &live
            ),
            Err(ContentFault::Corrupt { .. })
        ));
    }

    #[test]
    fn a_dead_target_is_refused() {
        let live = live(&["pack.test:item/crown-of-dusk"]);

        assert!(matches!(
            table(
                &[("pack.test:item/crown-of-dusk-old", "pack.test:item/missing")],
                &live
            ),
            Err(ContentFault::Corrupt { .. })
        ));
    }

    #[test]
    fn a_duplicate_alias_is_refused() {
        let live = live(&["pack.test:item/crown-of-dusk"]);
        let rows = [
            (
                "pack.test:item/crown-of-dusk-old",
                "pack.test:item/crown-of-dusk",
            ),
            (
                "pack.test:item/crown-of-dusk-old",
                "pack.test:item/crown-of-dusk",
            ),
        ];

        assert!(matches!(
            table(&rows, &live),
            Err(ContentFault::Corrupt { .. })
        ));
    }
}
