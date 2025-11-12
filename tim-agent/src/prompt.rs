use serde::Serialize;
use tinytemplate::TinyTemplate;

pub fn render(template: &str, ctx: &impl Serialize) -> Result<String, tinytemplate::error::Error> {
    let mut tt = TinyTemplate::new();
    tt.add_template("prompt", template)?;
    let rendered = tt.render("prompt", ctx)?;
    Ok(rendered.trim().to_string())
}
