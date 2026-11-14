import Testing

@testable import NavreeShell

@Test func theRowIsShownOnlyWhenTheAppOffersIt() {
    #expect(LoginItemRules.row(status: .enabled, offered: false) == nil)
    #expect(LoginItemRules.row(status: .notRegistered, offered: true) == LoginRow(on: false, needsApproval: false))
    #expect(LoginItemRules.row(status: .notFound, offered: true) == LoginRow(on: false, needsApproval: false))
}

@Test func aRegisteredItemIsOn() {
    #expect(LoginItemRules.row(status: .enabled, offered: true) == LoginRow(on: true, needsApproval: false))
}

@Test func anItemAwaitingApprovalSendsThePersonToSystemSettings() {
    #expect(LoginItemRules.row(status: .requiresApproval, offered: true) == LoginRow(on: true, needsApproval: true))
}

@Test func openingAtLoginIsNeverOfferedUnlessTheAppSaysSo() {
    #expect(!Presence().offersOpenAtLogin)
}
