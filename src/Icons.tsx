import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

function Icon({ children, ...props }: IconProps) {
  return (
    <svg
      className="ui-icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {children}
    </svg>
  );
}

export const CloseIcon = (props: IconProps) => <Icon {...props}><path d="m6 6 12 12M18 6 6 18" /></Icon>;
export const PlusIcon = (props: IconProps) => <Icon {...props}><path d="M12 5v14M5 12h14" /></Icon>;
export const CheckIcon = (props: IconProps) => <Icon {...props}><path d="m5 12 4 4L19 6" /></Icon>;
export const AlertIcon = (props: IconProps) => <Icon {...props}><path d="M12 8v5M12 17h.01" /></Icon>;
export const InfoIcon = (props: IconProps) => <Icon {...props}><circle cx="12" cy="12" r="9" /><path d="M12 11v6M12 7h.01" /></Icon>;
export const BoltIcon = (props: IconProps) => <Icon {...props}><path d="m13 2-8 12h7l-1 8 8-12h-7l1-8Z" /></Icon>;
export const DownloadIcon = (props: IconProps) => <Icon {...props}><path d="M12 3v12m0 0 4-4m-4 4-4-4M5 20h14" /></Icon>;
export const UploadIcon = (props: IconProps) => <Icon {...props}><path d="M12 17V5m0 0 4 4m-4-4L8 9M5 20h14" /></Icon>;
export const SettingsIcon = (props: IconProps) => <Icon {...props}><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1.03 1.56V21h-4v-.08A1.7 1.7 0 0 0 9 19.37a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.63 15 1.7 1.7 0 0 0 3.08 14H3v-4h.08A1.7 1.7 0 0 0 4.63 9a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.63 1.7 1.7 0 0 0 10 3.08V3h4v.08A1.7 1.7 0 0 0 15 4.63a1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.37 9 1.7 1.7 0 0 0 20.92 10H21v4h-.08A1.7 1.7 0 0 0 19.4 15Z" /></Icon>;
export const EditIcon = (props: IconProps) => <Icon {...props}><path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4L16.5 3.5Z" /></Icon>;
export const TrashIcon = (props: IconProps) => <Icon {...props}><path d="M4 7h16M9 7V4h6v3m3 0-1 14H7L6 7m4 4v6m4-6v6" /></Icon>;
export const CopyIcon = (props: IconProps) => <Icon {...props}><rect x="8" y="8" width="11" height="11" rx="1.5" /><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" /></Icon>;
export const GripIcon = (props: IconProps) => <Icon {...props}><circle cx="9" cy="6" r="1" fill="currentColor" stroke="none" /><circle cx="15" cy="6" r="1" fill="currentColor" stroke="none" /><circle cx="9" cy="12" r="1" fill="currentColor" stroke="none" /><circle cx="15" cy="12" r="1" fill="currentColor" stroke="none" /><circle cx="9" cy="18" r="1" fill="currentColor" stroke="none" /><circle cx="15" cy="18" r="1" fill="currentColor" stroke="none" /></Icon>;
export const LinkIcon = (props: IconProps) => <Icon {...props}><path d="M10 13a5 5 0 0 0 7.5.5l2-2a5 5 0 0 0-7-7l-1.15 1.15M14 11a5 5 0 0 0-7.5-.5l-2 2a5 5 0 0 0 7 7l1.15-1.15" /></Icon>;
export const MessageIcon = (props: IconProps) => <Icon {...props}><path d="M21 12a8 8 0 0 1-8 8H6l-4 2 1.4-4A9 9 0 1 1 21 12Z" /><path d="M8 12h.01M12 12h.01M16 12h.01" /></Icon>;
export const ChevronDownIcon = (props: IconProps) => <Icon {...props}><path d="m7 10 5 5 5-5" /></Icon>;
export const ChevronRightIcon = (props: IconProps) => <Icon {...props}><path d="m10 7 5 5-5 5" /></Icon>;
export const InboxIcon = (props: IconProps) => <Icon {...props}><path d="M4 5h16l2 9v5H2v-5l2-9Z" /><path d="M2 14h5l2 3h6l2-3h5" /></Icon>;
export const FolderIcon = (props: IconProps) => <Icon {...props}><path d="M3 6h7l2 2h9v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6Z" /><path d="M3 10h18" /></Icon>;
export const ClockIcon = (props: IconProps) => <Icon {...props}><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></Icon>;
export const RefreshIcon = (props: IconProps) => <Icon {...props}><path d="M20 7v5h-5" /><path d="M4 17v-5h5" /><path d="M6.1 9a7 7 0 0 1 11.6-2L20 12M4 12l2.3 5a7 7 0 0 0 11.6-2" /></Icon>;
export const BookOpenIcon = (props: IconProps) => <Icon {...props}><path d="M3 5.5A3.5 3.5 0 0 1 6.5 2H11v17H6.5A3.5 3.5 0 0 0 3 22V5.5Z" /><path d="M21 5.5A3.5 3.5 0 0 0 17.5 2H13v17h4.5A3.5 3.5 0 0 1 21 22V5.5Z" /></Icon>;
