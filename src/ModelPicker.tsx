import {
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { CheckIcon, ChevronDownIcon } from "./Icons";

interface ModelPickerProps {
  value: string;
  models: string[];
  placeholder?: string;
  ariaLabel: string;
  disabled?: boolean;
  onChange: (value: string) => void;
}

type MenuPosition = {
  style: CSSProperties;
  placement: "above" | "below";
};

const VIEWPORT_MARGIN = 12;
const MENU_GAP = 6;
const MENU_MAX_HEIGHT = 304;
const MENU_MIN_WIDTH = 320;

export function ModelPicker({
  value,
  models,
  placeholder,
  ariaLabel,
  disabled = false,
  onChange,
}: ModelPickerProps) {
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const [position, setPosition] = useState<MenuPosition | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuId = useId();
  const query = value.trim().toLowerCase();
  const hasExactMatch = models.some((item) => item.toLowerCase() === query);
  const filtered = useMemo(
    () => models
      .filter((item) => !query || hasExactMatch || item.toLowerCase().includes(query))
      .slice(0, 100),
    [hasExactMatch, models, query],
  );

  function updatePosition() {
    const anchor = rootRef.current;
    if (!anchor) return;

    const rect = anchor.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;
    if (rect.bottom < VIEWPORT_MARGIN || rect.top > viewportHeight - VIEWPORT_MARGIN) {
      setOpen(false);
      return;
    }
    const roomBelow = viewportHeight - rect.bottom - VIEWPORT_MARGIN - MENU_GAP;
    const roomAbove = rect.top - VIEWPORT_MARGIN - MENU_GAP;
    const placement = roomBelow < 220 && roomAbove > roomBelow ? "above" : "below";
    const availableHeight = Math.max(120, placement === "above" ? roomAbove : roomBelow);
    const maxHeight = Math.min(MENU_MAX_HEIGHT, availableHeight);
    const width = Math.min(
      Math.max(rect.width, MENU_MIN_WIDTH),
      viewportWidth - VIEWPORT_MARGIN * 2,
    );
    const left = Math.min(
      Math.max(rect.left, VIEWPORT_MARGIN),
      viewportWidth - width - VIEWPORT_MARGIN,
    );

    setPosition({
      placement,
      style: {
        left,
        width,
        maxHeight,
        ...(placement === "above"
          ? { bottom: viewportHeight - rect.top + MENU_GAP }
          : { top: rect.bottom + MENU_GAP }),
      },
    });
  }

  useLayoutEffect(() => {
    if (!open) {
      setPosition(null);
      return;
    }
    updatePosition();
  }, [open, models.length]);

  useEffect(() => {
    if (!open) return;

    const selected = filtered.findIndex((item) => item === value);
    setActiveIndex(selected >= 0 ? selected : 0);

    function closeOnOutsideClick(event: PointerEvent) {
      const target = event.target as Node;
      if (!rootRef.current?.contains(target) && !menuRef.current?.contains(target)) {
        setOpen(false);
      }
    }

    window.addEventListener("pointerdown", closeOnOutsideClick);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      window.removeEventListener("pointerdown", closeOnOutsideClick);
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [filtered, open, value]);

  useEffect(() => {
    if (!open) return;
    menuRef.current
      ?.querySelector<HTMLElement>(`[data-model-index="${activeIndex}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, open]);

  useEffect(() => {
    if (disabled || models.length === 0) setOpen(false);
  }, [disabled, models.length]);

  function chooseModel(model: string) {
    onChange(model);
    setOpen(false);
  }

  function handleKeyDown(event: ReactKeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") {
      setOpen(false);
      return;
    }
    if ((event.key === "ArrowDown" || event.key === "ArrowUp") && models.length) {
      event.preventDefault();
      if (!open) {
        setOpen(true);
        return;
      }
      const direction = event.key === "ArrowDown" ? 1 : -1;
      setActiveIndex((current) => {
        const count = filtered.length;
        return count ? (current + direction + count) % count : 0;
      });
      return;
    }
    if (event.key === "Enter" && open && filtered[activeIndex]) {
      event.preventDefault();
      chooseModel(filtered[activeIndex]);
    }
  }

  const menu = open && models.length > 0 && position && (
    <div
      ref={menuRef}
      id={menuId}
      className={`model-menu ${position.placement}`}
      style={position.style}
      role="listbox"
      aria-label={`${ariaLabel}可用模型`}
      onMouseDown={(event) => event.stopPropagation()}
    >
      <div className="model-menu-head">
        <span>选择 · {ariaLabel}</span>
        <span>{filtered.length}/{models.length}</span>
      </div>
      <div className="model-options">
        {filtered.length > 0 ? (
          filtered.map((item, index) => (
            <button
              type="button"
              role="option"
              aria-selected={item === value}
              className={
                "model-option" +
                (item === value ? " selected" : "") +
                (index === activeIndex ? " active" : "")
              }
              data-model-index={index}
              key={item}
              title={item}
              onMouseEnter={() => setActiveIndex(index)}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => chooseModel(item)}
            >
              <span>{item}</span>
              {item === value && <CheckIcon className="ui-icon model-check" />}
            </button>
          ))
        ) : (
          <div className="model-empty">没有匹配模型，可继续输入自定义名称</div>
        )}
      </div>
    </div>
  );

  return (
    <div className={"model-picker" + (open ? " open" : "")} ref={rootRef}>
      <input
        className="mono"
        value={value}
        role="combobox"
        aria-label={ariaLabel}
        aria-autocomplete="list"
        aria-controls={open ? menuId : undefined}
        aria-expanded={open}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(event) => {
          onChange(event.target.value);
          if (models.length) setOpen(true);
        }}
        onFocus={() => !disabled && models.length && setOpen(true)}
        onKeyDown={handleKeyDown}
      />
      <button
        type="button"
        className="model-trigger"
        aria-label={disabled ? "测试进行中" : models.length ? "展开模型列表" : "请先拉取模型"}
        aria-controls={open ? menuId : undefined}
        aria-expanded={open}
        title={disabled ? "测试进行中" : models.length ? `选择已拉取的模型（${models.length}）` : "请先拉取模型"}
        disabled={disabled || !models.length}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => setOpen((current) => !current)}
      >
        <span className="model-count">{models.length || "—"}</span>
        <ChevronDownIcon className="ui-icon model-chevron" />
      </button>
      {typeof document !== "undefined" && menu ? createPortal(menu, document.body) : null}
    </div>
  );
}
