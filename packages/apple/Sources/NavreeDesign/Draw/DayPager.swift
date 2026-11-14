import NavreeProtocol
import SwiftUI

/// The day a screen is about: the day's small score rings, its name in words
/// over its date, between the steps to the day before and the day after. A
/// tap on the day opens the system's calendar bounded by the days there are.
/// Each step is the page's own address sent as it came; a chosen day is sent
/// with the pager's `pick` and the day as the event's text. The core changes
/// the screen in place: the stack does not grow.
public struct DayPagerView: View {
    let pager: Navree.DayPager
    @Environment(\.navree) private var context
    @State private var choosing = false

    public init(pager: Navree.DayPager) {
        self.pager = pager
    }

    public var body: some View {
        #if os(iOS)
        // Under the title or at the head of the content, the day leads at the
        // column's edge, as the title's words do, and the steps stand at the
        // trailing end.
        HStack(spacing: 0) {
            day
            Spacer(minLength: 8)
            step(pager.previous, symbol: "chevron.backward", words: pager.words?.previous ?? "Previous day")
            step(pager.next, symbol: "chevron.forward", words: pager.words?.next ?? "Next day")
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .sheet(isPresented: $choosing) {
            DayChooser(pager: pager)
        }
        #else
        HStack(spacing: 4) {
            step(pager.previous, symbol: "chevron.backward", words: pager.words?.previous ?? "Previous day")
            day
            step(pager.next, symbol: "chevron.forward", words: pager.words?.next ?? "Next day")
        }
        .sheet(isPresented: $choosing) {
            DayChooser(pager: pager)
        }
        #endif
    }

    /// The day's rings and its name over its date; a tap opens the calendar.
    private var day: some View {
        Button {
            choosing = true
        } label: {
            HStack(spacing: 8) {
                if let scores = pager.scores { NodeView(scores) }
                VStack(alignment: Self.wordsAlignment, spacing: 0) {
                    Text(pager.day).font(.headline)
                    Text(PagerWords.date(pager.date))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .lineLimit(1)
                .fixedSize()
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(pager.pick == nil)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(pager.day), \(PagerWords.date(pager.date))")
        .accessibilityHint(pager.words?.choose ?? "")
    }

    /// The day's words lead with the control on iPhone and iPad; centred in a Mac's bar.
    #if os(iOS)
    private static let wordsAlignment = HorizontalAlignment.leading
    #else
    private static let wordsAlignment = HorizontalAlignment.center
    #endif

    /// The side of a step's target: on iPhone and iPad the 44 points Apple
    /// gives every control a finger taps; on a Mac a pointer's smaller one.
    #if os(iOS)
    private static let stepSide: CGFloat = 44
    #else
    private static let stepSide: CGFloat = 32
    #endif

    private func step(_ href: String?, symbol: String, words: String) -> some View {
        Button {
            if let href { context.send(.act(.navigate(Navree.Action.Navigate(href: href)), Place())) }
        } label: {
            Label(words, systemImage: symbol)
                .labelStyle(.iconOnly)
                .fontWeight(.semibold)
                .frame(minWidth: Self.stepSide, minHeight: Self.stepSide)
                .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
        .disabled(href == nil)
    }
}

/// The system's calendar for choosing a day, in a sheet with Cancel and Done.
private struct DayChooser: View {
    let pager: Navree.DayPager
    @Environment(\.navree) private var context
    @Environment(\.dismiss) private var dismiss
    @State private var day: Date?

    var body: some View {
        NavigationStack {
            Group {
                if let bounds = PagerRules.bounds(pager) {
                    DatePicker(
                        pager.words?.choose ?? "",
                        selection: Binding(get: { day ?? PagerRules.date(pager.date) ?? Date() }, set: { day = $0 }),
                        in: bounds,
                        displayedComponents: .date
                    )
                } else {
                    DatePicker(
                        pager.words?.choose ?? "",
                        selection: Binding(get: { day ?? PagerRules.date(pager.date) ?? Date() }, set: { day = $0 }),
                        displayedComponents: .date
                    )
                }
            }
            .datePickerStyle(.graphical)
            .environment(\.timeZone, TimeZone(identifier: "UTC")!)
            .padding(.horizontal)
            .navigationTitle(pager.words?.choose ?? "")
            .navigationBarTitleDisplayModeInline()
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(role: .cancel) { dismiss() } label: { Label("Cancel", systemImage: "xmark") }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(role: .confirm) {
                        if let pick = pager.pick, let day {
                            let text = PagerRules.text(day)
                            if PagerRules.choosable(text, first: pager.first, last: pager.last) {
                                context.send(.act(.navigate(Navree.Action.Navigate(href: pick.href)), Place(text: text)))
                            }
                        }
                        dismiss()
                    } label: {
                        Label("Done", systemImage: "checkmark")
                    }
                    .disabled(day == nil)
                }
            }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }
}

/// A day of the pager in the person's words.
enum PagerWords {
    static func date(_ text: String) -> String {
        guard let date = PagerRules.date(text) else { return text }
        var style = Date.FormatStyle.dateTime.weekday(.abbreviated).day().month(.abbreviated)
        style.timeZone = TimeZone(identifier: "UTC")!
        return date.formatted(style)
    }
}

/// The screen's day control in the navigation bar, beside the screen's
/// title, never in its place. On iPhone and iPad, under a large title, where
/// the title's subtitle stands, with the day in words beside the title once
/// it folds inline; under an inline title the control heads the content
/// instead (the screen draws it there). On a Mac, the bar's principal item.
/// A "today" step stands at the trailing end when another day is shown.
public struct PagerInBar: ViewModifier {
    let pager: Navree.DayPager?
    let place: PagerPlace
    @Environment(\.navree) private var context

    public init(pager: Navree.DayPager?, place: PagerPlace = .underTitle) {
        self.pager = pager
        self.place = place
    }

    public func body(content: Content) -> some View {
        if let pager {
            #if os(iOS)
            content
                .toolbar {
                    if place == .underTitle {
                        ToolbarItem(placement: .largeSubtitle) {
                            DayPagerView(pager: pager)
                        }
                        ToolbarItem(placement: .subtitle) {
                            Text(PagerRules.words(pager))
                        }
                    }
                    today(pager)
                }
            #else
            content
                .navigationBarTitleDisplayModeInline()
                .toolbar {
                    ToolbarItem(placement: .principal) {
                        DayPagerView(pager: pager)
                    }
                    today(pager)
                }
            #endif
        } else {
            content
        }
    }

    @ToolbarContentBuilder private func today(_ pager: Navree.DayPager) -> some ToolbarContent {
        if let today = pager.today {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    context.send(.act(.navigate(Navree.Action.Navigate(href: today)), Place()))
                } label: {
                    Label("Today", systemImage: "calendar.badge.clock")
                }
            }
        }
    }
}
