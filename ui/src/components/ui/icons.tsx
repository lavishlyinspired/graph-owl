import * as React from "react";
import { cn } from "../../lib/utils";

interface IconProps extends React.SVGProps<SVGSVGElement> {
  size?: number;
}

const Icon = React.forwardRef<SVGSVGElement, IconProps & { children: React.ReactNode }>(
  ({ size = 16, className, children, ...props }, ref) => (
    <svg
      ref={ref}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={cn("shrink-0", className)}
      {...props}
    >
      {children}
    </svg>
  ),
);
Icon.displayName = "Icon";

export const ApartmentOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M3 21h18" />
      <path d="M5 21V7l8-4 8 4v14" />
      <path d="M9 21v-6h6v6" />
      <path d="M10 9h4" />
      <path d="M10 12h4" />
      <path d="M10 15h4" />
    </Icon>
  ),
);
ApartmentOutlined.displayName = "ApartmentOutlined";

export const ShareAltOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <circle cx="18" cy="5" r="3" />
      <circle cx="6" cy="12" r="3" />
      <circle cx="18" cy="19" r="3" />
      <path d="M8.59 13.51 15.42 17.49" />
      <path d="M15.41 6.51 8.59 10.49" />
    </Icon>
  ),
);
ShareAltOutlined.displayName = "ShareAltOutlined";

export const ArrowLeftOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="m12 19-7-7 7-7" />
      <path d="M19 12H5" />
    </Icon>
  ),
);
ArrowLeftOutlined.displayName = "ArrowLeftOutlined";

export const BookOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20" />
    </Icon>
  ),
);
BookOutlined.displayName = "BookOutlined";

export const CalendarOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <rect width="18" height="18" x="3" y="4" rx="2" ry="2" />
      <path d="M16 2v4" />
      <path d="M8 2v4" />
      <path d="M3 10h18" />
    </Icon>
  ),
);
CalendarOutlined.displayName = "CalendarOutlined";

export const ReconciliationOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
      <path d="m9 15 2 2 4-4" />
    </Icon>
  ),
);
ReconciliationOutlined.displayName = "ReconciliationOutlined";

export const BulbOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-1 1.5-2 1.5-3.5A6 6 0 0 0 6 8c0 1.5.5 2.5 1.5 3.5.8.8 1.3 1.5 1.5 2.5" />
      <path d="M9 18h6" />
      <path d="M10 22h4" />
    </Icon>
  ),
);
BulbOutlined.displayName = "BulbOutlined";

export const DownloadOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <path d="M7 10 12 15 17 10" />
      <path d="M12 15V3" />
    </Icon>
  ),
);
DownloadOutlined.displayName = "DownloadOutlined";

export const CheckCircleFilled = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <svg
      ref={ref}
      width={props.size ?? 16}
      height={props.size ?? 16}
      viewBox="0 0 24 24"
      fill="currentColor"
      stroke="none"
      className={cn("shrink-0", props.className)}
      {...props}
    >
      <path d="M12 22c5.523 0 10-4.477 10-10S17.523 2 12 2 2 6.477 2 12s4.477 10 10 10Z" />
      <path
        d="m9 12 2 2 4-4"
        stroke="white"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  ),
);
CheckCircleFilled.displayName = "CheckCircleFilled";

export const ClockCircleOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <circle cx="12" cy="12" r="10" />
      <path d="M12 6v6l4 2" />
    </Icon>
  ),
);
ClockCircleOutlined.displayName = "ClockCircleOutlined";

export const CloudServerOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z" />
      <path d="M8 15h8" />
      <path d="M8 18h8" />
      <path d="M10 12h.01" />
      <path d="M14 12h.01" />
    </Icon>
  ),
);
CloudServerOutlined.displayName = "CloudServerOutlined";

export const CompassOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <circle cx="12" cy="12" r="10" />
      <path d="m16.24 7.76-2.12 6.36-6.36 2.12 2.12-6.36 6.36-2.12z" />
    </Icon>
  ),
);
CompassOutlined.displayName = "CompassOutlined";

export const DashboardOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <rect width="7" height="9" x="3" y="3" rx="1" />
      <rect width="7" height="5" x="14" y="3" rx="1" />
      <rect width="7" height="5" x="14" y="12" rx="1" />
      <rect width="7" height="9" x="3" y="12" rx="1" />
    </Icon>
  ),
);
DashboardOutlined.displayName = "DashboardOutlined";

export const DatabaseOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <ellipse cx="12" cy="5" rx="9" ry="3" />
      <path d="M3 5V19A9 3 0 0 0 21 19V5" />
      <path d="M3 12A9 3 0 0 0 21 12" />
    </Icon>
  ),
);
DatabaseOutlined.displayName = "DatabaseOutlined";

export const FolderOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </Icon>
  ),
);
FolderOutlined.displayName = "FolderOutlined";

export const PlusOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M5 12h14" />
      <path d="M12 5v14" />
    </Icon>
  ),
);
PlusOutlined.displayName = "PlusOutlined";

export const SafetyCertificateOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
      <path d="m9 12 2 2 4-4" />
    </Icon>
  ),
);
SafetyCertificateOutlined.displayName = "SafetyCertificateOutlined";

export const SearchOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <circle cx="11" cy="11" r="8" />
      <path d="m21 21-4.3-4.3" />
    </Icon>
  ),
);
SearchOutlined.displayName = "SearchOutlined";

export const TableOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <rect width="18" height="18" x="3" y="3" rx="2" ry="2" />
      <path d="M3 9h18" />
      <path d="M3 15h18" />
      <path d="M12 3v18" />
    </Icon>
  ),
);
TableOutlined.displayName = "TableOutlined";

export const TagOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M12 2H2v10l9 9 10-10-9-9Z" />
      <circle cx="7" cy="7" r="1.5" fill="currentColor" stroke="none" />
    </Icon>
  ),
);
TagOutlined.displayName = "TagOutlined";

export const ThunderboltOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z" />
    </Icon>
  ),
);
ThunderboltOutlined.displayName = "ThunderboltOutlined";

export const EditOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
      <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
    </Icon>
  ),
);
EditOutlined.displayName = "EditOutlined";

export const HistoryOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M3 3v5h5" />
      <path d="M3.05 13A9 9 0 1 0 6 5.3L3 8" />
      <path d="M12 7v5l4 2" />
    </Icon>
  ),
);
HistoryOutlined.displayName = "HistoryOutlined";

export const MergeOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M8 3v18" />
      <path d="M16 3v6" />
      <path d="M16 15v6" />
      <path d="m8 12 4-4 4 4" />
    </Icon>
  ),
);
MergeOutlined.displayName = "MergeOutlined";

export const TeamOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
      <circle cx="9" cy="7" r="4" />
      <path d="M22 21v-2a4 4 0 0 0-3-3.87" />
      <path d="M16 3.13a4 4 0 0 1 0 7.75" />
    </Icon>
  ),
);
TeamOutlined.displayName = "TeamOutlined";

export const UserOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2" />
      <circle cx="12" cy="7" r="4" />
    </Icon>
  ),
);
UserOutlined.displayName = "UserOutlined";

export const RobotOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <rect width="18" height="14" x="3" y="6" rx="2" />
      <path d="M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2" />
      <circle cx="9" cy="13" r="1.5" fill="currentColor" stroke="none" />
      <circle cx="15" cy="13" r="1.5" fill="currentColor" stroke="none" />
      <path d="M10 17h4" />
    </Icon>
  ),
);
RobotOutlined.displayName = "RobotOutlined";

export const CloseOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M18 6 6 18" />
      <path d="m6 6 12 12" />
    </Icon>
  ),
);
CloseOutlined.displayName = "CloseOutlined";

export const ExclamationCircleOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <circle cx="12" cy="12" r="10" />
      <path d="M12 8v4" />
      <path d="M12 16h.01" />
    </Icon>
  ),
);
ExclamationCircleOutlined.displayName = "ExclamationCircleOutlined";

export const InfoCircleOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <circle cx="12" cy="12" r="10" />
      <path d="M12 16v-4" />
      <path d="M12 8h.01" />
    </Icon>
  ),
);
InfoCircleOutlined.displayName = "InfoCircleOutlined";

export const CheckCircleOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <circle cx="12" cy="12" r="10" />
      <path d="m9 12 2 2 4-4" />
    </Icon>
  ),
);
CheckCircleOutlined.displayName = "CheckCircleOutlined";

export const WarningOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z" />
      <path d="M12 9v4" />
      <path d="M12 17h.01" />
    </Icon>
  ),
);
WarningOutlined.displayName = "WarningOutlined";

export const MoreOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <circle cx="12" cy="12" r="1" fill="currentColor" stroke="none" />
      <circle cx="19" cy="12" r="1" fill="currentColor" stroke="none" />
      <circle cx="5" cy="12" r="1" fill="currentColor" stroke="none" />
    </Icon>
  ),
);
MoreOutlined.displayName = "MoreOutlined";

export const UploadOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <path d="M17 8l-5-5-5 5" />
      <path d="M12 3v12" />
    </Icon>
  ),
);
UploadOutlined.displayName = "UploadOutlined";

export const DeleteOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M3 6h18" />
      <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
      <path d="M10 11v6" />
      <path d="M14 11v6" />
    </Icon>
  ),
);
DeleteOutlined.displayName = "DeleteOutlined";

export const SettingOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.47a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.39a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
      <circle cx="12" cy="12" r="3" />
    </Icon>
  ),
);
SettingOutlined.displayName = "SettingOutlined";

export const ReloadOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
      <path d="M3 3v5h5" />
    </Icon>
  ),
);
ReloadOutlined.displayName = "ReloadOutlined";

export const FileOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M14.5 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7.5L14.5 2z" />
      <path d="M14 2v6h6" />
    </Icon>
  ),
);
FileOutlined.displayName = "FileOutlined";

export const FilterOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
    </Icon>
  ),
);
FilterOutlined.displayName = "FilterOutlined";

export const SortAscendingOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M11 12h9" />
      <path d="M11 18h7" />
      <path d="M11 6h5" />
      <path d="M4 14h2.5" />
      <path d="M4 6h2.5" />
      <path d="M4 18h2.5" />
      <path d="M4 10V4" />
      <path d="M7 7 4 4 1 7" />
    </Icon>
  ),
);
SortAscendingOutlined.displayName = "SortAscendingOutlined";

export const SortDescendingOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M11 12h9" />
      <path d="M11 18h7" />
      <path d="M11 6h5" />
      <path d="M4 14h2.5" />
      <path d="M4 6h2.5" />
      <path d="M4 18h2.5" />
      <path d="M4 10v6" />
      <path d="M7 17 4 20 1 17" />
    </Icon>
  ),
);
SortDescendingOutlined.displayName = "SortDescendingOutlined";

export const EllipsisOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <circle cx="12" cy="12" r="1" fill="currentColor" stroke="none" />
      <circle cx="19" cy="12" r="1" fill="currentColor" stroke="none" />
      <circle cx="5" cy="12" r="1" fill="currentColor" stroke="none" />
    </Icon>
  ),
);
EllipsisOutlined.displayName = "EllipsisOutlined";

export const DownOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="m6 9 6 6 6-6" />
    </Icon>
  ),
);
DownOutlined.displayName = "DownOutlined";

export const UpOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="m18 15-6-6-6 6" />
    </Icon>
  ),
);
UpOutlined.displayName = "UpOutlined";

export const RightOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="m9 18 6-6-6-6" />
    </Icon>
  ),
);
RightOutlined.displayName = "RightOutlined";

export const LeftOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="m15 18-6-6 6-6" />
    </Icon>
  ),
);
LeftOutlined.displayName = "LeftOutlined";

export const CaretDownOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <svg
      ref={ref}
      width={props.size ?? 16}
      height={props.size ?? 16}
      viewBox="0 0 24 24"
      fill="currentColor"
      stroke="none"
      className={cn("shrink-0", props.className)}
      {...props}
    >
      <path d="M12 16 4 8h16l-8 8z" />
    </svg>
  ),
);
CaretDownOutlined.displayName = "CaretDownOutlined";

export const CaretRightOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <svg
      ref={ref}
      width={props.size ?? 16}
      height={props.size ?? 16}
      viewBox="0 0 24 24"
      fill="currentColor"
      stroke="none"
      className={cn("shrink-0", props.className)}
      {...props}
    >
      <path d="M8 4v16l12-8-12-8z" />
    </svg>
  ),
);
CaretRightOutlined.displayName = "CaretRightOutlined";

export const MenuOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M4 5h16" />
      <path d="M4 12h16" />
      <path d="M4 19h16" />
    </Icon>
  ),
);
MenuOutlined.displayName = "MenuOutlined";

export const CloseCircleFilled = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <svg
      ref={ref}
      width={props.size ?? 16}
      height={props.size ?? 16}
      viewBox="0 0 24 24"
      fill="currentColor"
      stroke="none"
      className={cn("shrink-0", props.className)}
      {...props}
    >
      <path d="M12 22c5.523 0 10-4.477 10-10S17.523 2 12 2 2 6.477 2 12s4.477 10 10 10Z" />
      <path
        d="m15 9-6 6"
        stroke="white"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
      <path
        d="m9 9 6 6"
        stroke="white"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
    </svg>
  ),
);
CloseCircleFilled.displayName = "CloseCircleFilled";

export const QuestionCircleOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <circle cx="12" cy="12" r="10" />
      <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
      <path d="M12 17h.01" />
    </Icon>
  ),
);
QuestionCircleOutlined.displayName = "QuestionCircleOutlined";

export const EyeOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z" />
      <circle cx="12" cy="12" r="3" />
    </Icon>
  ),
);
EyeOutlined.displayName = "EyeOutlined";

export const LinkOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
      <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
    </Icon>
  ),
);
LinkOutlined.displayName = "LinkOutlined";

export const ExportOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <path d="M7 10 12 15 17 10" />
      <path d="M12 15V3" />
    </Icon>
  ),
);
ExportOutlined.displayName = "ExportOutlined";

export const SaveOutlined = React.forwardRef<SVGSVGElement, IconProps>(
  (props, ref) => (
    <Icon ref={ref} {...props}>
      <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z" />
      <path d="M17 21v-8H7v8" />
      <path d="M7 3v5h8" />
    </Icon>
  ),
);
SaveOutlined.displayName = "SaveOutlined";
