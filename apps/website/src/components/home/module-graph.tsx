/**
 * The homepage's module graph: platform modules on the left export their
 * providers, `AppModule` (the root, holding the DI container) imports every
 * module, and your feature modules on the right get what they declare
 * injected. Hand-drawn on an 8 px grid: elbow wires meet each box at the
 * centre of a side, arrowheads point where providers go, and `AppModule` is
 * the one accent element. Two pulses travel the wiring (hidden under
 * reduced motion). On phones it re-lays out as a vertical stack.
 */

type Node = { name: string; note: string };

const platform: Node[] = [
  { name: "HttpModule", note: "routes, pipeline" },
  { name: "DatabaseModule", note: "Db, Uow, migrations" },
  { name: "AuthModule", note: "tokens, CurrentUser" },
  { name: "QueueModule", note: "jobs and workers" },
  { name: "WorkspaceModule", note: "tenants, members" },
];

const features: Node[] = [
  { name: "DealsModule", note: "DealService, routes" },
  { name: "CompaniesModule", note: "CompanyService" },
  { name: "ReportsModule", note: "imports DealsModule" },
  { name: "BillingModule", note: "plans, invoices" },
];

const TITLE = "How a Navree app is wired";
const DESC =
  "Five platform modules (HttpModule, DatabaseModule, AuthModule, QueueModule and WorkspaceModule) export their providers into AppModule, the root module that holds the dependency-injection container. The container injects them into four feature modules: DealsModule, CompaniesModule, ReportsModule and BillingModule. The graph is checked when the app boots.";

// Geometry (viewBox units, 8 px grid).
const CHIP_W = 176;
const CHIP_H = 48;
const LEFT_Y = platform.map((_, i) => 64 + i * 64); // centres
const RIGHT_Y = features.map((_, i) => 96 + i * 64);
const MID = 192;
const CARD = { x: 240, y: 128, w: 160, h: 128 };
const LBUS = 208;
const RBUS = 432;
const RX = 464;

function Chip({ x, cy, node }: { x: number; cy: number; node: Node }) {
  return (
    <g>
      <rect x={x + 0.5} y={cy - CHIP_H / 2 + 0.5} width={CHIP_W - 1} height={CHIP_H - 1} rx={10} className="fill-ink-2 stroke-line-2" />
      <text x={x + 16} y={cy - 3} className="fill-fg-1 font-mono text-[13px]">
        {node.name}
      </text>
      <text x={x + 16} y={cy + 14} className="fill-fg-3 text-[12px]">
        {node.note}
      </text>
    </g>
  );
}

function Arrow({ x, y }: { x: number; y: number }) {
  return <path d={`M${x} ${y} l-7 -4 v8 z`} className="graph-head" />;
}

function Pulse({ path, begin }: { path: string; begin: string }) {
  return (
    <circle r={3} className="graph-pulse" opacity={0}>
      <animateMotion path={path} dur="6s" begin={begin} repeatCount="indefinite" keyPoints="0;1;1" keyTimes="0;0.5;1" calcMode="linear" />
      <animate attributeName="opacity" values="0;1;1;0;0" keyTimes="0;0.04;0.46;0.5;1" dur="6s" begin={begin} repeatCount="indefinite" />
    </circle>
  );
}

function DesktopGraph() {
  return (
    <svg viewBox="0 0 640 352" role="img" aria-labelledby="module-graph-title module-graph-desc" className="h-auto w-full overflow-visible">
      <title id="module-graph-title">{TITLE}</title>
      <desc id="module-graph-desc">{DESC}</desc>

      {/* Column captions */}
      <text x={0} y={16} className="fill-fg-3 font-mono text-[12px] tracking-[0.12em]">
        PLATFORM · EXPORTS
      </text>
      <text x={RX} y={16} className="fill-fg-3 font-mono text-[12px] tracking-[0.12em]">
        YOUR APP · IMPORTS
      </text>

      {/* Left wiring: stubs into a bus, the bus into AppModule. */}
      {LEFT_Y.map((y) => (
        <path key={y} d={`M${CHIP_W} ${y} H${LBUS}`} className="graph-wire" />
      ))}
      <path d={`M${LBUS} ${LEFT_Y[0]} V${LEFT_Y[LEFT_Y.length - 1]}`} className="graph-wire" />
      <path d={`M${LBUS} ${MID} H${CARD.x - 7}`} className="graph-wire" />
      <Arrow x={CARD.x} y={MID} />

      {/* Right wiring: AppModule into a bus, the bus into each feature module. */}
      <path d={`M${CARD.x + CARD.w} ${MID} H${RBUS}`} className="graph-wire" />
      <path d={`M${RBUS} ${RIGHT_Y[0]} V${RIGHT_Y[RIGHT_Y.length - 1]}`} className="graph-wire" />
      {RIGHT_Y.map((y) => (
        <g key={y}>
          <path d={`M${RBUS} ${y} H${RX - 7}`} className="graph-wire" />
          <Arrow x={RX} y={y} />
        </g>
      ))}

      {/* Pulses run under the card, which hides them as they cross it. */}
      <Pulse path={`M${CHIP_W} ${LEFT_Y[1]} H${LBUS} V${MID} H${RBUS} V${RIGHT_Y[0]} H${RX - 2}`} begin="0.6s" />
      <Pulse path={`M${CHIP_W} ${LEFT_Y[4]} H${LBUS} V${MID} H${RBUS} V${RIGHT_Y[3]} H${RX - 2}`} begin="3.6s" />

      {platform.map((node, i) => (
        <Chip key={node.name} x={0} cy={LEFT_Y[i]} node={node} />
      ))}
      {features.map((node, i) => (
        <Chip key={node.name} x={RX} cy={RIGHT_Y[i]} node={node} />
      ))}

      {/* AppModule: the one accent element. */}
      <rect x={CARD.x - 6} y={CARD.y - 6} width={CARD.w + 12} height={CARD.h + 12} rx={18} className="fill-none stroke-accent/15" />
      <rect x={CARD.x + 0.5} y={CARD.y + 0.5} width={CARD.w - 1} height={CARD.h - 1} rx={12} className="fill-ink-1 stroke-accent" />
      <rect x={CARD.x + 1} y={CARD.y + 1} width={CARD.w - 2} height={CARD.h - 2} rx={11.5} className="fill-accent/[0.06]" />
      <text x={CARD.x + CARD.w / 2} y={MID - 18} textAnchor="middle" className="fill-fg font-mono text-[15px] font-medium">
        AppModule
      </text>
      <text x={CARD.x + CARD.w / 2} y={MID + 6} textAnchor="middle" className="fill-fg-2 text-[12px]">
        type-keyed container
      </text>
      <text x={CARD.x + CARD.w / 2} y={MID + 26} textAnchor="middle" className="fill-accent text-[12px]">
        checked at boot
      </text>
    </svg>
  );
}

function StackChip({ node }: { node: Node }) {
  return (
    <li className="rounded-[10px] border border-line-2 bg-ink-2 px-3 py-2.5">
      <span className="block truncate font-mono text-[0.8125rem] text-fg-1">{node.name}</span>
      <span className="mt-0.5 block truncate text-xs text-fg-3">{node.note}</span>
    </li>
  );
}

function Down() {
  return (
    <div aria-hidden className="relative mx-auto h-8 w-px bg-line-2">
      <span className="absolute -bottom-px left-1/2 size-0 -translate-x-1/2 border-x-4 border-t-[7px] border-x-transparent border-t-line-2" />
    </div>
  );
}

function PhoneGraph() {
  // role="img" makes the boxes presentational: the label says it all.
  return (
    <div role="img" aria-label={`${TITLE}. ${DESC}`} className="flex flex-col gap-3">
      <p className="font-mono text-xs tracking-[0.12em] text-fg-3">PLATFORM · EXPORTS</p>
      <ul className="grid grid-cols-2 gap-2">
        {platform.map((node) => (
          <StackChip key={node.name} node={node} />
        ))}
      </ul>
      <Down />
      <div className="rounded-xl border border-accent bg-accent/[0.06] px-4 py-4 text-center shadow-[0_0_0_5px_rgb(61_255_143/0.08)]">
        <span className="block font-mono text-[0.9375rem] font-medium text-fg">AppModule</span>
        <span className="mt-1 block text-xs text-fg-2">type-keyed container</span>
        <span className="mt-0.5 block text-xs text-accent">checked at boot</span>
      </div>
      <Down />
      <p className="font-mono text-xs tracking-[0.12em] text-fg-3">YOUR APP · IMPORTS</p>
      <ul className="grid grid-cols-2 gap-2">
        {features.map((node) => (
          <StackChip key={node.name} node={node} />
        ))}
      </ul>
    </div>
  );
}

export function ModuleGraph() {
  return (
    <>
      {/* Drawn at 640 units: shown only where it renders at least that wide, so text stays at 12 px or more. */}
      <div className="hidden md:block">
        <DesktopGraph />
      </div>
      <div className="md:hidden">
        <PhoneGraph />
      </div>
    </>
  );
}
