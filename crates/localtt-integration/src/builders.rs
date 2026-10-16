//! Builders and constructor sugar, re-exported from [`localtt_core::builders`].
//!
//! The definitions live in core (RFC-0003 sections 4 and 5.2). This module is a
//! facade so the scenario suite has a single, stable consumer surface; when the
//! builders graduate to `localtt-engine`, only this re-export changes.

pub use localtt_core::builders::{
    CampaignBuilder,
    StatblockBuilder,
    content_ref,
    dice,
    ref_id,
    stat,
};
