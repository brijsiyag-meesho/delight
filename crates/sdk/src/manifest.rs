//! What a plugin declares about itself: its identity, logo and operations.

/// Static description of a plugin and the operations it offers.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PluginManifest {
    /// Globally unique, reverse-DNS style id, e.g. `delight.json`.
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    /// The tool's logo: an SVG document, usually `include_bytes!("icon.svg")`.
    /// Drawn in full colour, as-is, at badge size — like an app icon, so give
    /// it its own background if it should read in both light and dark mode.
    /// Draw it on a square viewBox. `'static` because plugin libraries are
    /// never unloaded.
    pub icon_svg: &'static [u8],
    /// Free-form tags. Also fed to model-based classifiers as label hints.
    pub tags: Vec<String>,
    pub operations: Vec<OperationSpec>,
}

impl PluginManifest {
    /// A manifest with an id, a name and a logo (see
    /// [`PluginManifest::icon_svg`]); add the rest with the builder methods
    /// below.
    pub fn new(id: impl Into<String>, name: impl Into<String>, icon_svg: &'static [u8]) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: String::new(),
            description: String::new(),
            author: None,
            icon_svg,
            tags: Vec::new(),
            operations: Vec::new(),
        }
    }

    pub fn operation(&self, id: &str) -> Option<&OperationSpec> {
        self.operations.iter().find(|op| op.id == id)
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }
    pub fn tags<S: Into<String>>(mut self, tags: impl IntoIterator<Item = S>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }
    pub fn operations(mut self, operations: impl IntoIterator<Item = OperationSpec>) -> Self {
        self.operations = operations.into_iter().collect();
        self
    }
}

/// One thing a plugin can do with an input, e.g. "Decode Base64".
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OperationSpec {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Label hints for model-based classification (e.g. `["base64", "decode"]`).
    pub tags: Vec<String>,
}

impl OperationSpec {
    /// An operation with just an id and a title; add the rest with the
    /// builder methods below.
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            tags: Vec::new(),
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
    pub fn tags<S: Into<String>>(mut self, tags: impl IntoIterator<Item = S>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }
}
