/** Compatibility barrel: Ant Design component names backed by shadcn/Tailwind
 *  primitives. Lets existing feature files swap `from "antd"` for a relative
 *  import without rewriting every JSX call site. */

export { App } from "./app";
export { Alert, AlertError, AlertInfo, AlertSuccess, AlertWarning } from "./alert";
export { AutoComplete } from "./autocomplete";
export { Badge, Tag } from "./badge";
export { Breadcrumb } from "./breadcrumb";
export { Button, buttonVariants } from "./button";
export { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "./card";
export { ConfirmButton } from "./confirm-button";
export { Descriptions } from "./descriptions";
export { Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger } from "./dialog";
export { Divider } from "./divider";
export { Drawer } from "./drawer";
export { Dropdown } from "./dropdown";
export { Empty } from "./empty";
export { Form, FormControl, FormDescription, FormItem, FormLabel, FormMessage } from "./form";
export {
  ApartmentOutlined,
  ArrowLeftOutlined,
  BookOutlined,
  BulbOutlined,
  CalendarOutlined,
  CaretDownOutlined,
  CaretRightOutlined,
  CheckCircleFilled,
  CheckCircleOutlined,
  ClockCircleOutlined,
  CloseCircleFilled,
  CloseOutlined,
  CloudServerOutlined,
  CompassOutlined,
  DashboardOutlined,
  DatabaseOutlined,
  DeleteOutlined,
  DownOutlined,
  DownloadOutlined,
  EditOutlined,
  EllipsisOutlined,
  ExclamationCircleOutlined,
  ExportOutlined,
  EyeOutlined,
  FileOutlined,
  FilterOutlined,
  FolderOutlined,
  HistoryOutlined,
  InfoCircleOutlined,
  LeftOutlined,
  LinkOutlined,
  MenuOutlined,
  MergeOutlined,
  MoreOutlined,
  PlusOutlined,
  QuestionCircleOutlined,
  ReconciliationOutlined,
  ReloadOutlined,
  RightOutlined,
  RobotOutlined,
  SafetyCertificateOutlined,
  SaveOutlined,
  SearchOutlined,
  SettingOutlined,
  ShareAltOutlined,
  SortAscendingOutlined,
  SortDescendingOutlined,
  TableOutlined,
  TagOutlined,
  TeamOutlined,
  ThunderboltOutlined,
  UpOutlined,
  UploadOutlined,
  UserOutlined,
  WarningOutlined,
} from "./icons";
export { Input } from "./input";
export { Label } from "./label";
export { Layout, Header, Sider, Content, Footer } from "./layout";
export { List } from "./list";
export { Menu } from "./menu";
export { Modal } from "./modal";
export { message, ToastProvider, useToast } from "./toast";
export { Popconfirm } from "./popconfirm";
export { AntPopover as Popover, PopoverContent, PopoverTrigger } from "./popover";
export { Segmented } from "./segmented";
export { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./select";
export { Space, Flex, Row, Col } from "./space";
export { Spin } from "./spin";
export { Statistic } from "./statistic";
export { Switch } from "./switch";
export { AntTable as Table, TableBody, TableCaption, TableCell, TableHead, TableHeader, TableRow } from "./table";
export { Tabs, TabsContent, TabsList, TabsTrigger } from "./tabs";
export { Textarea } from "./textarea";
export { Timeline } from "./timeline";
export { AntTooltip as Tooltip, TooltipProvider } from "./tooltip";
export { Tree } from "./tree";
export { Typography, Text, Title, Paragraph } from "./typography";
export { Upload } from "./upload";

// Types that some files import directly from antd/es/*.
export type { TreeDataNode } from "./tree";
export type { UploadFile } from "./upload";
