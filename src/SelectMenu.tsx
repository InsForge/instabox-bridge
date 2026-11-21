import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import type { KeyboardEvent, ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { Check } from './icons';

type Option = { value: string; label: string; content: ReactNode };

/** A select with rich options, kept inside the desktop window. Focus stays on its trigger. */
export default function SelectMenu({ label, value, options, onChange, children, className, disabled = false, above = false }: {
  label: string; value: string; options: Option[]; onChange: (value: string) => void;
  children: ReactNode; className: string; disabled?: boolean; above?: boolean;
}) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [layoutVersion, setLayoutVersion] = useState(0);
  const [position, setPosition] = useState<{ top: number; left: number; maxHeight: number; maxWidth: number } | null>(null);
  const search = useRef({ text: '', time: 0 });
  const container = trigger.current?.closest<HTMLElement>('.popover');
  const selected = Math.max(0, options.findIndex((option) => option.value === value));
  const show = () => { setActive(selected); setPosition(null); setOpen(true); };
  const choose = (index: number) => {
    const option = options[index];
    if (!option) return;
    setOpen(false);
    trigger.current?.focus();
    if (option.value !== value) onChange(option.value);
  };

  useLayoutEffect(() => {
    if (!open || !container || !menu.current || !trigger.current) return;
    const a = trigger.current.getBoundingClientRect(), c = container.getBoundingClientRect();
    const edge = 8, gap = 6;
    const width = container.clientWidth, height = container.clientHeight;
    const maxHeight = Math.max(40, height - edge * 2);
    const h = Math.min(menu.current.offsetHeight, maxHeight);
    const w = Math.min(menu.current.offsetWidth, width - edge * 2);
    const below = a.bottom - c.top - container.clientTop + gap;
    const up = a.top - c.top - container.clientTop - gap - h;
    const top = above || below + h > height - edge ? up : below;
    const left = above ? a.left - c.left - container.clientLeft : a.right - c.left - container.clientLeft - w;
    setPosition({ top: Math.max(edge, Math.min(top, height - edge - h)),
      left: Math.max(edge, Math.min(left, width - edge - w)), maxHeight, maxWidth: width - edge * 2 });
  }, [open, container, above, options.length, layoutVersion]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: Event) => {
      const node = event.target as Node;
      if (!trigger.current?.contains(node) && !menu.current?.contains(node)) setOpen(false);
    };
    const resize = () => setLayoutVersion((version) => version + 1);
    document.addEventListener('pointerdown', outside);
    window.addEventListener('resize', resize);
    return () => {
      document.removeEventListener('pointerdown', outside);
      window.removeEventListener('resize', resize);
    };
  }, [open]);

  useEffect(() => {
    const list = menu.current;
    const option = list?.children[active] as HTMLElement | undefined;
    if (!open || !list || !option) return;
    if (option.offsetTop < list.scrollTop) list.scrollTop = option.offsetTop;
    else if (option.offsetTop + option.offsetHeight > list.scrollTop + list.clientHeight)
      list.scrollTop = option.offsetTop + option.offsetHeight - list.clientHeight;
  }, [open, active, position]);

  const keyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === 'Tab') { setOpen(false); return; }
    if (event.key === 'Escape') { if (open) { event.preventDefault(); event.stopPropagation(); setOpen(false); } return; }
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      if (!open) { show(); return; }
      setActive(event.key === 'Home' ? 0 : event.key === 'End' ? options.length - 1 :
        (active + (event.key === 'ArrowDown' ? 1 : -1) + options.length) % options.length);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (open) choose(active); else show();
    } else if (event.key.length === 1 && !event.metaKey && !event.ctrlKey && !event.altKey) {
      const now = Date.now();
      search.current = { text: (now - search.current.time < 700 ? search.current.text : '') + event.key.toLowerCase(), time: now };
      const index = options.findIndex((option) => option.label.toLowerCase().startsWith(search.current.text));
      if (index >= 0) { if (!open) show(); setActive(index); }
    }
  };

  return <>
    <button ref={trigger} type="button" role="combobox" className={className}
      aria-label={label} aria-expanded={open} aria-haspopup="listbox" aria-controls={open ? id : undefined}
      aria-activedescendant={open ? `${id}-${active}` : undefined} disabled={disabled}
      title={options[selected]?.label} onKeyDown={keyDown} onBlur={(event) => {
        if (event.relatedTarget && !menu.current?.contains(event.relatedTarget)) setOpen(false);
      }}
      onClick={() => open ? setOpen(false) : show()}>
      {children}
    </button>
    {open && container && createPortal(
      <div ref={menu} id={id} role="listbox" aria-label={label} className="selection-menu"
        style={position ?? { visibility: 'hidden', top: 0, left: 0 }} onMouseDown={(event) => event.preventDefault()}>
        {options.map((option, index) => <div key={option.value} id={`${id}-${index}`} role="option"
          aria-label={option.label} aria-selected={option.value === value} className="selection-option"
          data-active={index === active} onPointerMove={() => setActive(index)} onClick={() => choose(index)}>
          {option.content}
          <span className="selection-check" aria-hidden="true">{option.value === value && <Check />}</span>
        </div>)}
      </div>, container)}
  </>;
}
