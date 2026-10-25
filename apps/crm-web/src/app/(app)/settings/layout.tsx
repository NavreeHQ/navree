import { SettingsNav } from "./settings-nav";

export default function SettingsLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex flex-col">
      <header className="flex flex-col gap-4 pb-6">
        <h1 className="text-[26px] leading-tight font-semibold tracking-[-0.015em] font-condensed">Settings</h1>
        <SettingsNav />
      </header>
      {children}
    </div>
  );
}
