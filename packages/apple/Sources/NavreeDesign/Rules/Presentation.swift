import NavreeProtocol

/// How a sheet of a given size is presented.
public enum SheetPresentation: Sendable, Equatable {
    /// A sheet that rests at half height and can be pulled up, with a grabber.
    case resizable
    /// A sheet at full height.
    case large
    /// Over the whole screen: a camera, a long multi-step task, sign-in.
    case fullScreen
}

/// What a dialog of the tree becomes.
public enum DialogPresentation: Sendable, Equatable {
    /// The system's alert: information or a question with a few answers.
    case alert
    /// An action sheet anchored to what was tapped: a choice about an action
    /// the person started, such as confirming a destructive one.
    case confirmationDialog
    /// A sheet: the dialog holds a form or other parts, which an alert can't.
    case sheet
}

/// What a message the web would show as a toast becomes. Apple systems have
/// no toast: a message lives in the screen, or interrupts when it must.
public enum MessagePresentation: Sendable, Equatable {
    /// A line of status at the top of the content, with a symbol, that the
    /// person can dismiss; it goes when they leave the screen.
    case inlineStatus
    /// The system's alert: something failed and the person must know.
    case alert
}

/// What a person feels as a message arrives.
public enum Haptic: Sendable, Equatable {
    case success, warning, error
}

/// Where a screen's search field goes.
public enum SearchPlacement: Sendable, Equatable {
    /// A field under the title, always shown, filtering the list below it.
    case underTitle
    /// The trailing end of the toolbar, as on iPad and the Mac.
    case toolbar
}

public enum PresentationRules {
    /// How a sheet takes its size.
    public enum SheetSizing: Sendable, Equatable {
        /// By its detents: iPhone and iPad.
        case detents
        /// The system's size for a form: a Mac window's sheet, which
        /// otherwise takes its content's own height, and a list has none.
        case form
    }

    public static func sheetSizing(bar: ActionBar) -> SheetSizing {
        bar == .window ? .form : .detents
    }

    /// Medium sheets rest at half height and can be pulled up (iPhone's
    /// detents; a regular width shows a form sheet); large ones take the full
    /// height; full ones cover the screen.
    public static func sheet(_ size: Navree.SheetSize) -> SheetPresentation {
        switch size {
        case .medium: .resizable
        case .large: .large
        case .full: .fullScreen
        }
    }

    /// A dialog that holds parts (a form) is a sheet; one that only asks, with
    /// a destructive answer, is a confirmation dialog from its source; any
    /// other that only asks is an alert.
    public static func dialog(_ dialog: Navree.Dialog) -> DialogPresentation {
        if !dialog.children.isEmpty {
            return .sheet
        }
        let destructive = dialog.actions.contains { node in
            if case .button(let button) = node { return button.role == .destructive }
            return false
        }
        return destructive ? .confirmationDialog : .alert
    }

    /// Whether a destructive action asks first. One that can be undone never
    /// does: the person undoes it instead. One that can't, asks.
    public static func asksFirst(_ button: Navree.Button) -> Bool {
        guard button.role == .destructive || button.standard == .delete else { return false }
        return !button.undoable
    }

    /// The question a control asks before it acts: the server's own words,
    /// unless what it does can be undone, where Apple's guidelines let the
    /// person undo it rather than be asked.
    public static func question(_ button: Navree.Button) -> Navree.Question? {
        button.undoable ? nil : button.ask
    }

    /// A failure interrupts with an alert; anything else is a line of status
    /// in the screen. Never a floating toast.
    public static func message(_ tone: Navree.Tone) -> MessagePresentation {
        tone == .danger ? .alert : .inlineStatus
    }

    /// What the person feels as a message of this tone arrives.
    public static func haptic(_ tone: Navree.Tone) -> Haptic? {
        switch tone {
        case .success: .success
        case .warning: .warning
        case .danger: .error
        case .neutral, .accent, .info: nil
        }
    }

    /// On a compact width the field sits under the title, above the list it
    /// filters (the tab bar holds the bottom); on a regular width it sits at
    /// the trailing end of the toolbar.
    public static func search(width: WidthClass) -> SearchPlacement {
        width == .compact ? .underTitle : .toolbar
    }

    /// Whether the search field joins the bar only once the screen has first
    /// appeared. A field under the title does: given with a list that is there
    /// from the bar's very first layout (a screen kept from the last launch,
    /// drawn as the app opens), iOS lays the bar out with its large title
    /// already folded, though nothing was scrolled. Joining a moment later, the
    /// bar grows under the large title, as when the list comes from the server.
    public static func searchWaitsForFirstAppearance(_ placement: SearchPlacement) -> Bool {
        placement == .underTitle
    }
}
