import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import { cva, type VariantProps } from "class-variance-authority";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...values: ClassValue[]) {
  return twMerge(clsx(values));
}
// Owned, composable primitives following shadcn/ui's open component pattern.
const buttonVariants = cva(
  "rg-button inline-flex items-center justify-center gap-2 rounded-md text-[13px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground hover:bg-primary/90",
        outline: "border border-input bg-background hover:bg-muted",
        ghost: "hover:bg-muted",
        destructive: "bg-destructive text-white hover:bg-destructive/90",
      },
      size: { default: "h-9 px-3", sm: "h-8 px-2.5", icon: "size-9 p-0" },
    },
    defaultVariants: { variant: "outline", size: "default" },
  },
);
export const Button = React.forwardRef<
  HTMLButtonElement,
  React.ButtonHTMLAttributes<HTMLButtonElement> &
    VariantProps<typeof buttonVariants> & { asChild?: boolean }
>(({ className, variant, size, asChild, type = "button", ...props }, ref) => {
  const Component = asChild ? Slot : "button";
  return (
    <Component
      ref={ref}
      type={type}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  );
});
Button.displayName = "Button";
export const Input = React.forwardRef<
  HTMLInputElement,
  React.InputHTMLAttributes<HTMLInputElement>
>(({ className, ...props }, ref) => (
  <input
    ref={ref}
    className={cn(
      "rg-input h-9 w-full rounded-md border border-input bg-background px-3 text-[13px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-50",
      className,
    )}
    {...props}
  />
));
Input.displayName = "Input";
export const NativeSelect = React.forwardRef<
  HTMLSelectElement,
  React.SelectHTMLAttributes<HTMLSelectElement>
>(({ className, ...props }, ref) => (
  <select
    ref={ref}
    className={cn(
      "rg-input h-9 rounded-md border border-input bg-background px-3 text-[13px]",
      className,
    )}
    {...props}
  />
));
NativeSelect.displayName = "NativeSelect";
export function Field({
  label,
  help,
  children,
}: {
  label: string;
  help?: string;
  children: React.ReactElement<{ id?: string }>;
}) {
  const id = React.useId();
  return (
    <div className="rg-field grid gap-2 text-xs font-medium text-muted-foreground">
      <div className="flex items-center gap-1.5">
        <label htmlFor={id}>{label}</label>
        {help && <Help label={label}>{help}</Help>}
      </div>
      {React.cloneElement(children, { id })}
    </div>
  );
}
export function CheckboxField({
  label,
  help,
  ...props
}: React.InputHTMLAttributes<HTMLInputElement> & {
  label: string;
  help: string;
}) {
  const id = React.useId();
  return (
    <div className="rg-check">
      <input {...props} id={id} type="checkbox" />
      <label htmlFor={id}>{label}</label>
      <Help label={label}>{help}</Help>
    </div>
  );
}
export function Help({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  const [open, setOpen] = React.useState(false);
  return (
    <TooltipPrimitive.Provider delayDuration={200}>
      <TooltipPrimitive.Root open={open} onOpenChange={setOpen}>
        <TooltipPrimitive.Trigger asChild>
          <button
            type="button"
            className="rg-help"
            aria-label={`About ${label}`}
            onClick={(event) => {
              event.preventDefault();
              setOpen(!open);
            }}
          >
            ?
          </button>
        </TooltipPrimitive.Trigger>
        <TooltipPrimitive.Portal>
          <TooltipPrimitive.Content
            sideOffset={6}
            collisionPadding={12}
            className="rg-tooltip z-[100] max-w-72 rounded-md bg-foreground px-3 py-2 text-xs leading-relaxed text-background shadow-lg"
          >
            {children}
          </TooltipPrimitive.Content>
        </TooltipPrimitive.Portal>
      </TooltipPrimitive.Root>
    </TooltipPrimitive.Provider>
  );
}
export function Dialog({
  title,
  description,
  children,
  footer,
  onClose,
  restoreFocus,
  initialFocus,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
  footer?: React.ReactNode;
  onClose: () => void;
  restoreFocus?: HTMLElement | null;
  initialFocus?: React.RefObject<HTMLElement | null>;
}) {
  const descriptionId = React.useId();
  return (
    <DialogPrimitive.Root open onOpenChange={onClose}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="rg-dialog-overlay fixed inset-0 z-50 bg-black/40" />
        <DialogPrimitive.Content
          onOpenAutoFocus={(event) => {
            if (initialFocus?.current) {
              event.preventDefault();
              initialFocus.current.focus();
            }
          }}
          aria-describedby={description ? descriptionId : undefined}
          className="rg-dialog fixed left-1/2 top-1/2 z-50 flex max-h-[calc(100dvh-32px)] w-[min(640px,calc(100vw-32px))] -translate-x-1/2 -translate-y-1/2 flex-col rounded-xl border border-border bg-background shadow-xl"
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            restoreFocus?.focus();
          }}
        >
          <header className="flex items-start justify-between gap-4 border-b border-border px-5 py-4">
            <div>
              <DialogPrimitive.Title className="text-sm font-medium text-foreground">
                {title}
              </DialogPrimitive.Title>
              {description && (
                <DialogPrimitive.Description
                  id={descriptionId}
                  className="mt-1 text-xs text-muted-foreground"
                >
                  {description}
                </DialogPrimitive.Description>
              )}
            </div>
            <DialogPrimitive.Close asChild>
              <Button variant="ghost" size="icon" aria-label="Close dialog">
                ×
              </Button>
            </DialogPrimitive.Close>
          </header>
          <div className="min-h-0 overflow-y-auto px-5 py-4">{children}</div>
          {footer && (
            <footer className="flex flex-wrap gap-2 border-t border-border px-5 py-3">
              {footer}
            </footer>
          )}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
