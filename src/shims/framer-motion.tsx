/* eslint-disable react-refresh/only-export-components */
import type { HTMLAttributes, ReactNode } from 'react'

type PresenceProps = {
  children: ReactNode
  mode?: 'wait' | 'sync' | 'popLayout'
}

type MotionProps = HTMLAttributes<HTMLElement> & {
  whileHover?: unknown
  initial?: unknown
  animate?: unknown
  exit?: unknown
  transition?: unknown
}

export function AnimatePresence({ children }: PresenceProps) {
  return <>{children}</>
}

function MotionElement({ children, ...props }: MotionProps) {
  return <article {...props}>{children}</article>
}

function MotionDiv({ children, ...props }: MotionProps) {
  return <div {...props}>{children}</div>
}

export const motion = {
  article: MotionElement,
  div: MotionDiv,
}
