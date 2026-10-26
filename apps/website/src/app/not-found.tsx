import Link from "next/link";

export default function NotFound() {
  return (
    <div className="mx-auto flex w-full max-w-[90rem] flex-1 flex-col justify-center px-4 py-24 sm:px-6 lg:px-8">
      <p className="text-sm text-fg-3">404</p>
      <h1 className="mt-2 text-4xl font-semibold tracking-[-0.03em] text-fg">This page isn&apos;t here.</h1>
      <p className="mt-4 max-w-md text-fg-2">
        The link may be old, or the page may have moved. Search the docs with <kbd className="kbd">/</kbd>, or start from one of these.
      </p>
      <div className="mt-8 flex flex-wrap gap-3">
        <Link href="/" className="btn btn-secondary">
          Home
        </Link>
        <Link href="/docs/" className="btn btn-secondary">
          Documentation
        </Link>
        <Link href="/ecosystem/" className="btn btn-secondary">
          Ecosystem
        </Link>
      </div>
    </div>
  );
}
