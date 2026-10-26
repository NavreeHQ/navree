"use client";

import {
  motion,
  useInView,
  type DOMMotionComponents,
  type HTMLMotionProps,
  type MotionProps,
} from "motion/react";
import {
  Children,
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ComponentType,
  type ReactNode,
  type RefAttributes,
} from "react";

import { cn } from "@/lib/utils";
import { useReducedMotion } from "@/lib/use-reduced-motion";

/*
 * Magic UI Terminal, restyled for Navree: an ink window with a title bar,
 * mono text, lines that type or fade in one after another. Under
 * prefers-reduced-motion every line shows at once, fully typed.
 *
 *   <Terminal title="~/blog">
 *     <TypingAnimation>{"$ navree new blog"}</TypingAnimation>
 *     <AnimatedSpan className="text-accent">✓ Created blog/</AnimatedSpan>
 *   </Terminal>
 */

interface SequenceContextValue {
  completeItem: (index: number) => void;
  activeIndex: number;
  sequenceStarted: boolean;
}

const SequenceContext = createContext<SequenceContextValue | null>(null);
const useSequence = () => useContext(SequenceContext);

const ItemIndexContext = createContext<number | null>(null);
const useItemIndex = () => useContext(ItemIndexContext);

const motionElements = {
  article: motion.article,
  div: motion.div,
  h1: motion.h1,
  h2: motion.h2,
  h3: motion.h3,
  h4: motion.h4,
  h5: motion.h5,
  h6: motion.h6,
  li: motion.li,
  p: motion.p,
  section: motion.section,
  span: motion.span,
} as const;

type MotionElementType = Extract<keyof DOMMotionComponents, keyof typeof motionElements>;
type TerminalTypingMotionComponent = ComponentType<Omit<HTMLMotionProps<"span">, "ref"> & RefAttributes<HTMLElement>>;

interface AnimatedSpanProps extends MotionProps {
  children: ReactNode;
  delay?: number;
  className?: string;
  startOnView?: boolean;
}

/** A line that fades in (output, a status line). */
export const AnimatedSpan = ({ children, delay = 0, className, startOnView = false, ...props }: AnimatedSpanProps) => {
  const elementRef = useRef<HTMLDivElement | null>(null);
  const isInView = useInView(elementRef as React.RefObject<Element>, { amount: 0.3, once: true });
  const reduced = useReducedMotion();

  const sequence = useSequence();
  const itemIndex = useItemIndex();
  // The active index only grows, so a line has started once it is reached.
  const hasStarted = !!sequence && itemIndex !== null && sequence.sequenceStarted && sequence.activeIndex >= itemIndex;

  const shouldAnimate = reduced || (sequence ? hasStarted : startOnView ? isInView : true);

  return (
    <motion.div
      ref={elementRef}
      initial={reduced ? false : { opacity: 0, y: -4 }}
      animate={shouldAnimate ? { opacity: 1, y: 0 } : { opacity: 0, y: -4 }}
      transition={{ duration: reduced ? 0 : 0.3, delay: sequence || reduced ? 0 : delay / 1000 }}
      className={cn("grid text-[0.8125rem] leading-relaxed font-normal text-fg-2", className)}
      onAnimationComplete={() => {
        if (!sequence) return;
        if (itemIndex === null) return;
        sequence.completeItem(itemIndex);
      }}
      {...props}
    >
      {children}
    </motion.div>
  );
};

interface TypingAnimationProps extends Omit<MotionProps, "children"> {
  children: string;
  className?: string;
  /** Milliseconds per character. */
  duration?: number;
  delay?: number;
  as?: MotionElementType;
  startOnView?: boolean;
}

/** A line that types itself out (a command). */
export const TypingAnimation = ({
  children,
  className,
  duration = 40,
  delay = 0,
  as: Component = "span",
  startOnView = true,
  ...props
}: TypingAnimationProps) => {
  const MotionComponent = motionElements[Component] as TerminalTypingMotionComponent;
  const reduced = useReducedMotion();

  const [displayedText, setDisplayedText] = useState<string>("");
  const [timerStarted, setTimerStarted] = useState(false);
  const elementRef = useRef<HTMLElement | null>(null);
  const isInView = useInView(elementRef as React.RefObject<Element>, { amount: 0.3, once: true });

  const sequence = useSequence();
  const itemIndex = useItemIndex();
  const hasSequence = sequence !== null;
  const sequenceStarted = sequence?.sequenceStarted ?? false;
  const sequenceActiveIndex = sequence?.activeIndex ?? null;
  const sequenceCompleteItemRef = useRef<SequenceContextValue["completeItem"] | null>(null);
  const sequenceItemIndexRef = useRef<number | null>(null);

  useEffect(() => {
    sequenceCompleteItemRef.current = sequence?.completeItem ?? null;
    sequenceItemIndexRef.current = itemIndex;
  }, [sequence?.completeItem, itemIndex]);

  const inSequence = hasSequence && itemIndex !== null;
  const started = inSequence
    ? sequenceStarted && sequenceActiveIndex !== null && sequenceActiveIndex >= itemIndex
    : timerStarted;

  useEffect(() => {
    if (inSequence || !(!startOnView || isInView)) return;
    const startTimeout = setTimeout(() => setTimerStarted(true), delay);
    return () => clearTimeout(startTimeout);
  }, [delay, startOnView, isInView, inSequence]);

  useEffect(() => {
    if (reduced) return;
    let typingEffect: ReturnType<typeof setInterval> | null = null;

    if (started) {
      let i = 0;
      typingEffect = setInterval(() => {
        if (i < children.length) {
          setDisplayedText(children.substring(0, i + 1));
          i++;
        } else {
          if (typingEffect !== null) clearInterval(typingEffect);
          const completeItem = sequenceCompleteItemRef.current;
          const currentItemIndex = sequenceItemIndexRef.current;
          if (completeItem && currentItemIndex !== null) completeItem(currentItemIndex);
        }
      }, duration);
    }

    return () => {
      if (typingEffect !== null) clearInterval(typingEffect);
    };
  }, [children, duration, started, reduced]);

  return (
    <MotionComponent
      ref={elementRef}
      className={cn("text-[0.8125rem] leading-relaxed font-normal text-fg", className)}
      {...props}
    >
      <span className="sr-only">{children}</span>
      <span aria-hidden>{reduced ? children : displayedText}</span>
    </MotionComponent>
  );
};

interface TerminalProps {
  children: ReactNode;
  className?: string;
  /** Shown in the title bar, e.g. `~/blog`. */
  title?: string;
  /** Play lines one after another (default) rather than all at once. */
  sequence?: boolean;
  startOnView?: boolean;
}

export const Terminal = ({ children, className, title, sequence = true, startOnView = true }: TerminalProps) => {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const isInView = useInView(containerRef as React.RefObject<Element>, { amount: 0.3, once: true });

  const [activeIndex, setActiveIndex] = useState(0);
  const sequenceHasStarted = sequence ? !startOnView || isInView : false;

  const contextValue = useMemo<SequenceContextValue | null>(() => {
    if (!sequence) return null;
    return {
      completeItem: (index: number) => {
        setActiveIndex((current) => (index === current ? current + 1 : current));
      },
      activeIndex,
      sequenceStarted: sequenceHasStarted,
    };
  }, [sequence, activeIndex, sequenceHasStarted]);

  const wrappedChildren = useMemo(() => {
    if (!sequence) return children;
    return Children.toArray(children).map((child, index) => (
      <ItemIndexContext.Provider key={index} value={index}>
        {child as ReactNode}
      </ItemIndexContext.Provider>
    ));
  }, [children, sequence]);

  const content = (
    <div
      ref={containerRef}
      className={cn("z-0 w-full overflow-hidden rounded-2xl border border-line bg-ink-1 shadow-bezel", className)}
    >
      <div className="flex h-10 items-center gap-3 border-b border-line px-4">
        <div className="flex gap-1.5" aria-hidden>
          <span className="size-2.5 rounded-full bg-line-2" />
          <span className="size-2.5 rounded-full bg-line-2" />
          <span className="size-2.5 rounded-full bg-line-2" />
        </div>
        {title ? <span className="font-mono text-xs text-fg-3">{title}</span> : null}
      </div>
      <pre className="scroll-thin overflow-x-auto p-4 sm:p-5">
        <code className="grid min-h-full gap-y-1">{wrappedChildren}</code>
      </pre>
    </div>
  );

  if (!sequence) return content;

  return <SequenceContext.Provider value={contextValue}>{content}</SequenceContext.Provider>;
};
