import NavreeShell
import Testing

@MainActor
@Test func thePreviewScreenBuilds() {
    _ = ShellPreview().body
}
