import { SettingsNav } from "./settings-nav";

export default function SettingsLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex flex-col">
      <header className="flex flex-col gap-5 pb-6">
        <div className="flex flex-col gap-2">
          <p className="eyebrow">Workspace</p>
          <h1 className="text-title font-semibold text-fg">Settings</h1>
        </div>
        <SettingsNav />
      </header>
      {children}
    </div>
  );
}
