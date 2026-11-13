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
        HStack(spacing: 4) {
            step(pager.previous, symbol: "chevron.backward", words: pager.words?.previous ?? "Previous day")
            Button {
                choosing = true
            } label: {
                HStack(spacing: 8) {
                    if let scores = pager.scores { NodeView(scores) }
                    VStack(spacing: 0) {
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
            step(pager.next, symbol: "chevron.forward", words: pager.words?.next ?? "Next day")
        }
        .sheet(isPresented: $choosing) {
            DayChooser(pager: pager)
        }
    }

    private func step(_ href: String?, symbol: String, words: String) -> some View {
        Button {
            if let href { context.send(.act(.navigate(Navree.Action.Navigate(href: href)), Place())) }
        } label: {
            Label(words, systemImage: symbol)
                .labelStyle(.iconOnly)
                .fontWeight(.semibold)
                .frame(minWidth: 32, minHeight: 32)
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

/// The screen's day control in the navigation bar: the bar's principal item,
/// with the screen's title inline for the back stack and VoiceOver, and a
/// "today" step at the trailing end when another day is shown.
public struct PagerInBar: ViewModifier {
    let pager: Navree.DayPager?
    @Environment(\.navree) private var context

    public init(pager: Navree.DayPager?) {
        self.pager = pager
    }

    public func body(content: Content) -> some View {
        if let pager {
            content
                .navigationBarTitleDisplayModeInline()
                .toolbar {
                    ToolbarItem(placement: .principal) {
                        DayPagerView(pager: pager)
                    }
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
        } else {
            content
        }
    }
}
