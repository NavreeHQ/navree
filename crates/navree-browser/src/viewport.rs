//! [`Viewport`]: a page's size, pixel ratio, touch support and user agent.

/// The size and device a page emulates.
///
/// | Preset | Size | Pixel ratio | Touch | User agent |
/// |---|---|---|---|---|
/// | [`Viewport::phone`] | 390 x 844 | 3 | yes | Chrome on an Android phone |
/// | [`Viewport::tablet`] | 820 x 1180 | 2 | yes | Chrome on an Android tablet |
/// | [`Viewport::desktop`] | 1440 x 900 | 1 | no | the browser's own |
///
/// [`Viewport::new`] is a desktop viewport of any size. Headless Chrome's
/// own user agent says `HeadlessChrome`; navree-browser sends `Chrome`
/// instead, so apps see the same browser as in production.
///
/// ```
/// use navree_browser::Viewport;
///
/// let phone = Viewport::phone();
/// assert_eq!((phone.width(), phone.height()), (390, 844));
/// assert!(phone.has_touch());
/// let wide = Viewport::new(1920, 1080);
/// assert_eq!(wide.scale(), 1.0);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    width: u32,
    height: u32,
    scale: f64,
    mobile: bool,
    touch: bool,
    agent: Agent,
}

/// Which user agent a viewport sends.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Agent {
    /// The browser's own (with `HeadlessChrome` replaced by `Chrome`).
    Browser,
    /// Chrome on an Android phone, with the browser's version.
    Phone,
    /// Chrome on an Android tablet, with the browser's version.
    Tablet,
    /// A user agent of the test's choosing.
    Custom(String),
}

impl Viewport {
    /// A desktop viewport of `width` by `height` CSS pixels, pixel ratio 1,
    /// no touch.
    pub fn new(width: u32, height: u32) -> Viewport {
        Viewport {
            width,
            height,
            scale: 1.0,
            mobile: false,
            touch: false,
            agent: Agent::Browser,
        }
    }

    /// 390 x 844, pixel ratio 3, touch, a mobile user agent.
    pub fn phone() -> Viewport {
        Viewport {
            width: 390,
            height: 844,
            scale: 3.0,
            mobile: true,
            touch: true,
            agent: Agent::Phone,
        }
    }

    /// 820 x 1180, pixel ratio 2, touch, a tablet user agent.
    pub fn tablet() -> Viewport {
        Viewport {
            width: 820,
            height: 1180,
            scale: 2.0,
            mobile: true,
            touch: true,
            agent: Agent::Tablet,
        }
    }

    /// 1440 x 900, pixel ratio 1, no touch: the default window.
    pub fn desktop() -> Viewport {
        Viewport::new(1440, 900)
    }

    /// The same viewport with another size.
    pub fn with_size(mut self, width: u32, height: u32) -> Viewport {
        self.width = width;
        self.height = height;
        self
    }

    /// The same viewport with another device pixel ratio.
    pub fn with_scale(mut self, scale: f64) -> Viewport {
        self.scale = scale;
        self
    }

    /// The same viewport with touch support on or off.
    pub fn with_touch(mut self, touch: bool) -> Viewport {
        self.touch = touch;
        self
    }

    /// The same viewport with another user agent.
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Viewport {
        self.agent = Agent::Custom(user_agent.into());
        self
    }

    /// The width in CSS pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The height in CSS pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The device pixel ratio.
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Whether the page sees a mobile device (`<meta name=viewport>` applies).
    pub fn is_mobile(&self) -> bool {
        self.mobile
    }

    /// Whether the page sees touch support.
    pub fn has_touch(&self) -> bool {
        self.touch
    }

    /// The user agent this viewport sends with the browser's `version`
    /// (`major.minor.build.patch`) and its own user agent `default`.
    pub(crate) fn user_agent(&self, version: &str, default: &str) -> String {
        let major = version.split('.').next().unwrap_or(version);
        match &self.agent {
            Agent::Browser => default.replace("HeadlessChrome", "Chrome"),
            Agent::Phone => format!(
                "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Mobile Safari/537.36"
            ),
            Agent::Tablet => format!(
                "Mozilla/5.0 (Linux; Android 14; SM-X710) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"
            ),
            Agent::Custom(agent) => agent.clone(),
        }
    }
}

impl Default for Viewport {
    fn default() -> Viewport {
        Viewport::desktop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_match_the_docs() {
        let phone = Viewport::phone();
        assert_eq!(
            (phone.width(), phone.height(), phone.scale()),
            (390, 844, 3.0)
        );
        assert!(phone.has_touch() && phone.is_mobile());
        let tablet = Viewport::tablet();
        assert_eq!((tablet.width(), tablet.height()), (820, 1180));
        assert!(tablet.has_touch());
        let desktop = Viewport::desktop();
        assert_eq!((desktop.width(), desktop.height()), (1440, 900));
        assert!(!desktop.has_touch() && !desktop.is_mobile());
        assert_eq!(Viewport::default(), desktop);
        assert_eq!(Viewport::new(1440, 900), desktop);
    }

    #[test]
    fn user_agents_carry_the_browser_version() {
        let headless = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/156.0.8078.48 Safari/537.36";
        assert_eq!(
            Viewport::desktop().user_agent("156.0.8078.48", headless),
            headless.replace("HeadlessChrome", "Chrome")
        );
        let phone = Viewport::phone().user_agent("156.0.8078.48", headless);
        assert!(
            phone.contains("Android") && phone.contains("Chrome/156.0.0.0 Mobile"),
            "{phone}"
        );
        let tablet = Viewport::tablet().user_agent("156.0.8078.48", headless);
        assert!(tablet.contains("Chrome/156.0.0.0 Safari"), "{tablet}");
        assert!(!tablet.contains("Mobile"), "{tablet}");
        assert_eq!(
            Viewport::new(800, 600)
                .with_user_agent("Robot/1.0")
                .user_agent("1", headless),
            "Robot/1.0"
        );
    }

    #[test]
    fn viewports_can_be_adjusted() {
        let custom = Viewport::phone()
            .with_size(360, 740)
            .with_scale(2.0)
            .with_touch(false);
        assert_eq!((custom.width(), custom.height()), (360, 740));
        assert_eq!(custom.scale(), 2.0);
        assert!(!custom.has_touch());
        assert!(custom.is_mobile());
    }
}
