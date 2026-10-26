//! Browser steps shared by more than one journey.

use anyhow::Context;

use super::browser::Browser;

/// The fields of the browser character-creation form.
pub(crate) struct Character<'a> {
    name: &'a str,
    class: &'a str,
    level: u32,
    max_hp: u32,
    abilities: &'a str,
}

impl<'a> Character<'a> {
    /// A level-1 fighter with 10 hit points and the standard array.
    pub(crate) fn new(name: &'a str) -> Self {
        Self {
            name,
            class: "Fighter",
            level: 1,
            max_hp: 10,
            abilities: "standard",
        }
    }

    /// Sets the class label.
    #[must_use]
    pub(crate) fn class(mut self, class: &'a str) -> Self {
        self.class = class;
        self
    }

    /// Sets the level.
    #[must_use]
    pub(crate) fn level(mut self, level: u32) -> Self {
        self.level = level;
        self
    }

    /// Sets the maximum hit points.
    #[must_use]
    pub(crate) fn max_hp(mut self, max_hp: u32) -> Self {
        self.max_hp = max_hp;
        self
    }

    /// Sets the ability scores field, as six comma-separated values or
    /// `standard`.
    #[must_use]
    pub(crate) fn abilities(mut self, abilities: &'a str) -> Self {
        self.abilities = abilities;
        self
    }
}

/// Creates a character and waits for its sheet to become the selection.
pub(crate) async fn create_character(
    browser: &Browser,
    character: &Character<'_>,
) -> anyhow::Result<()> {
    browser
        .click("[data-testid='character-create'] summary")
        .await
        .context("the character form opens")?;
    browser
        .fill("[data-testid='character-name']", character.name)
        .await?;
    browser
        .fill("[data-testid='character-class']", character.class)
        .await?;
    browser
        .fill(
            "[data-testid='character-level']",
            &character.level.to_string(),
        )
        .await?;
    browser
        .fill(
            "[data-testid='character-max-hp']",
            &character.max_hp.to_string(),
        )
        .await?;
    browser
        .fill("[data-testid='character-abilities']", character.abilities)
        .await?;
    browser
        .click("[data-testid='create-character-button']")
        .await
        .context("the character is submitted")?;
    browser.wait_idle().await?;
    browser
        .wait_text("[data-testid='entity-name']", character.name)
        .await
        .with_context(|| format!("{} is selected after creation", character.name))?;
    browser
        .wait_text(
            "[data-testid='hp-value']",
            &format!("{hp}/{hp}", hp = character.max_hp),
        )
        .await
        .with_context(|| format!("{} starts at full hit points", character.name))?;
    browser
        .wait_attr("[data-testid='entity-detail']", "data-writable", "true")
        .await
        .with_context(|| format!("{} is writable by the signed-in player", character.name))?;
    Ok(())
}
