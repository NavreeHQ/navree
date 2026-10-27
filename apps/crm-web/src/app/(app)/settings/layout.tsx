import { SettingsNav } from "./settings-nav";

export default function SettingsLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex flex-col">
      <header className="flex flex-col gap-4 pb-5">
        <div className="flex flex-col gap-1">
          <h1 className="text-title font-semibold text-fg">Settings</h1>
          <p className="text-sm text-fg-2">Your profile, the team and the workspace&apos;s history.</p>
        </div>
        <SettingsNav />
      </header>
      {children}
    </div>
  );
}
