//! Insertable snippets for the Scripts tab.
//!
//! These are editing conveniences, not executable behaviour: this build has no
//! script engine, which the Scripts tab states plainly. A template's *name* is a
//! label the user reads, so it is a [`Str`]; its *body* is script source in the
//! familiar `pm.*` shape and is kept verbatim, like any code the editor holds
//! (the same reason a code-editor language id or a header token is not
//! translated).

use crate::i18n::Str;

/// A snippet the Scripts tab can drop into the pre-request or post-response
/// editor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScriptTemplate {
    // Pre-request.
    SetHeader,
    SetBearerToken,
    SetTimestamp,
    // Post-response.
    AssertStatus,
    LogResponse,
    ExtractField,
}

impl ScriptTemplate {
    /// The templates offered for the pre-request editor.
    pub const PRE_REQUEST: &'static [ScriptTemplate] = &[
        ScriptTemplate::SetHeader,
        ScriptTemplate::SetBearerToken,
        ScriptTemplate::SetTimestamp,
    ];

    /// The templates offered for the post-response editor.
    pub const POST_RESPONSE: &'static [ScriptTemplate] = &[
        ScriptTemplate::AssertStatus,
        ScriptTemplate::LogResponse,
        ScriptTemplate::ExtractField,
    ];

    /// The menu label naming the template.
    pub fn label(self) -> Str {
        match self {
            ScriptTemplate::SetHeader => Str::TemplateSetHeader,
            ScriptTemplate::SetBearerToken => Str::TemplateSetBearerToken,
            ScriptTemplate::SetTimestamp => Str::TemplateSetTimestamp,
            ScriptTemplate::AssertStatus => Str::TemplateAssertStatus,
            ScriptTemplate::LogResponse => Str::TemplateLogResponse,
            ScriptTemplate::ExtractField => Str::TemplateExtractField,
        }
    }

    /// The script source inserted into the editor. Written in Postman's `pm.*`
    /// shape because that is what a user reaching for a template expects;
    /// nothing in this build runs it.
    pub fn snippet(self) -> &'static str {
        match self {
            ScriptTemplate::SetHeader => {
                "// Add a header to this request.\n\
                 pm.request.headers.add({ key: \"X-Example\", value: \"value\" });"
            }
            ScriptTemplate::SetBearerToken => {
                "// Attach a bearer token to this request.\n\
                 pm.request.headers.add({ key: \"Authorization\", \
                 value: \"Bearer \" + pm.variables.get(\"token\") });"
            }
            ScriptTemplate::SetTimestamp => {
                "// Store the current time in a variable for this request.\n\
                 pm.variables.set(\"timestamp\", Date.now());"
            }
            ScriptTemplate::AssertStatus => {
                "// Check that the response status is 200.\n\
                 pm.test(\"Status is 200\", function () {\n\
                 \x20   pm.response.to.have.status(200);\n\
                 });"
            }
            ScriptTemplate::LogResponse => {
                "// Print the response body to the console.\n\
                 console.log(pm.response.text());"
            }
            ScriptTemplate::ExtractField => {
                "// Read a field from the JSON response into a variable.\n\
                 const data = pm.response.json();\n\
                 pm.variables.set(\"id\", data.id);"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ScriptTemplate;

    #[test]
    fn every_offered_template_has_a_non_empty_snippet() {
        for template in ScriptTemplate::PRE_REQUEST
            .iter()
            .chain(ScriptTemplate::POST_RESPONSE)
        {
            assert!(
                !template.snippet().trim().is_empty(),
                "a template offers no snippet"
            );
        }
    }

    #[test]
    fn the_two_menus_do_not_share_a_template() {
        for pre in ScriptTemplate::PRE_REQUEST {
            assert!(
                !ScriptTemplate::POST_RESPONSE.contains(pre),
                "a template appears in both menus"
            );
        }
    }
}
