use serde::Serialize;
use tinytemplate::error::Error as TemplateError;

use crate::prompt::render as render_template;
use crate::tim_client::tim_api::{Ability as SpaceAbility, AbilityParameter, TimiteAbilities};

const SPACE_ABILITIES_TEMPLATE: &str = include_str!("../../prompts/space_abilities.txt");
const SPACE_ABILITY_ENTRY_TEMPLATE: &str = include_str!("../../prompts/space_ability_entry.txt");

#[derive(Serialize)]
struct AbilityEntryTemplateCtx {
    owner: String,
    name: String,
    description: String,
    params: String,
}

#[derive(Serialize)]
struct SpaceAbilitiesTemplateCtx<'a> {
    entries: &'a str,
}

pub(super) fn render_space_abilities(
    abilities: &[TimiteAbilities],
) -> Result<Option<String>, TemplateError> {
    let mut entries = Vec::new();
    for envelope in abilities {
        let owner = ability_owner(envelope);
        for ability in &envelope.abilities {
            if let Some(ctx) = ability_entry_ctx(&owner, ability) {
                entries.push(render_template(SPACE_ABILITY_ENTRY_TEMPLATE, &ctx)?);
            }
        }
    }
    if entries.is_empty() {
        return Ok(None);
    }
    let block = entries.join("\n");
    let ctx = SpaceAbilitiesTemplateCtx {
        entries: block.trim(),
    };
    let rendered = render_template(SPACE_ABILITIES_TEMPLATE, &ctx)?;
    Ok(Some(rendered))
}

fn ability_entry_ctx(owner: &str, ability: &SpaceAbility) -> Option<AbilityEntryTemplateCtx> {
    let name = ability.name.trim();
    if name.is_empty() {
        return None;
    }
    let description = ability.description.trim();
    Some(AbilityEntryTemplateCtx {
        owner: owner.to_string(),
        name: name.to_string(),
        description: if description.is_empty() {
            "no description provided".to_string()
        } else {
            description.to_string()
        },
        params: format_params(&ability.params),
    })
}

fn ability_owner(envelope: &TimiteAbilities) -> String {
    envelope
        .timite
        .as_ref()
        .map(|timite| {
            let nick = timite.nick.trim();
            if nick.is_empty() {
                format!("timite#{}", timite.id)
            } else {
                nick.to_string()
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_params(params: &[AbilityParameter]) -> String {
    if params.is_empty() {
        return "none".to_string();
    }
    params
        .iter()
        .map(|param| {
            let name = param.name.trim();
            let desc = param.description.trim();
            match (name.is_empty(), desc.is_empty()) {
                (true, true) => "value".to_string(),
                (true, false) => desc.to_string(),
                (false, true) => name.to_string(),
                (false, false) => format!("{name} ({desc})"),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
