import { AlertIcon, CheckIcon, InfoIcon } from "./Icons";

export type ToastKind = "success" | "error" | "info";
export interface Toast {
  id: number;
  kind: ToastKind;
  msg: string;
}

export default function Toasts({ toasts }: { toasts: Toast[] }) {
  return (
    <div className="toasts">
      {toasts.map((t) => (
        <div key={t.id} className={"toast " + t.kind}>
          <span className="ic">{t.kind === "success" ? <CheckIcon /> : t.kind === "error" ? <AlertIcon /> : <InfoIcon />}</span>
          <span>{t.msg}</span>
        </div>
      ))}
    </div>
  );
}
