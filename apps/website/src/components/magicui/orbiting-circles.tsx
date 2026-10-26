import { Children, type CSSProperties, type HTMLAttributes, type ReactNode } from "react";

import { cn } from "@/lib/utils";

export interface OrbitingCirclesProps extends HTMLAttributes<HTMLDivElement> {
  className?: string;
  children?: ReactNode;
  reverse?: boolean;
  /** Seconds per orbit (before `speed`). */
  duration?: number;
  delay?: number;
  /** Orbit radius, px. */
  radius?: number;
  /** Draw the orbit as a hairline circle. */
  path?: boolean;
  iconSize?: number;
  speed?: number;
}

/**
 * Items that circle a centre (the module hub: modules orbiting the DI
 * container). Put it in a `relative` square box with the centre item
 * absolutely centred. CSS animation, so it stops under
 * prefers-reduced-motion with every item still at its own angle.
 */
export function OrbitingCircles({
  className,
  children,
  reverse,
  duration = 30,
  radius = 160,
  path = true,
  iconSize = 40,
  speed = 1,
  ...props
}: OrbitingCirclesProps) {
  const calculatedDuration = duration / speed;
  const count = Children.count(children);
  return (
    <>
      {path ? (
        <svg aria-hidden xmlns="http://www.w3.org/2000/svg" version="1.1" className="pointer-events-none absolute inset-0 size-full">
          <circle className="stroke-line-2" strokeWidth={1} cx="50%" cy="50%" r={radius} fill="none" />
        </svg>
      ) : null}
      {Children.map(children, (child, index) => {
        const angle = (360 / count) * index;
        return (
          <div
            style={
              {
                "--duration": calculatedDuration,
                "--radius": radius,
                "--angle": angle,
                "--icon-size": `${iconSize}px`,
                transform: `rotate(${angle}deg) translateY(${radius}px) rotate(${-angle}deg)`,
              } as CSSProperties
            }
            className={cn(
              "absolute flex size-(--icon-size) transform-gpu animate-orbit items-center justify-center rounded-full motion-reduce:animate-none",
              reverse && "[animation-direction:reverse]",
              className,
            )}
            {...props}
          >
            {child}
          </div>
        );
      })}
    </>
  );
}
