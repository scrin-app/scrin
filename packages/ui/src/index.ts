export { cn, cx } from './lib/cn';
export { MotionProvider, useMotionDuration, useSpring } from './lib/motion';

export * from './theme/tokens';
export { ACCENT_PRESETS, presetFor, type AccentPreset } from './theme/presets';
export { contrastRatio, parseColor, formatOklch, type Oklch } from './theme/contrast';
export {
  ThemeProvider,
  useTheme,
  useOptionalTheme,
  THEME_STORAGE_KEY,
  type ThemeContextValue,
} from './theme/ThemeProvider';

export type * from './platform';
export { HostProvider, useHost, useOptionalHost } from './host/HostProvider';
export { createWebHost, createWebStorage, createMockEngine } from './host/web-host';

export { Button, IconButton, buttonVariants, type ButtonProps } from './components/button';
export { Spinner } from './components/spinner';
export { Field, Input, Label, inputClass } from './components/field';
export { Dialog, AlertDialog } from './components/dialog';
export { Popover, floatingClass } from './components/popover';
export {
  Menu,
  MenuItem,
  MenuSeparator,
  MenuGroup,
  MenuRadioGroup,
  MenuRadioItem,
} from './components/menu';
export { Tabs, TabsList, Tab, TabsPanel } from './components/tabs';
export { Switch, SwitchRow, Checkbox, RadioGroup, Segmented, Slider } from './components/controls';
export { Select, type SelectOption } from './components/select';
export { Tooltip, TooltipProvider } from './components/tooltip';
export { Toaster, toast } from './components/toast';
export {
  Skeleton,
  Progress,
  Badge,
  badgeVariants,
  StatusDot,
  Card,
  CardHeader,
  Kbd,
  EmptyState,
} from './components/display';
export { CodeDisplay, CountdownRing } from './components/code-display';
export { SasEmoji, SAS_EMOJI } from './components/sas-emoji';
export {
  PERMISSIONS,
  PERMISSION_LABEL_KEY,
  ANONYMOUS_FORBIDDEN,
  ANONYMOUS_MAX_MINUTES,
  defaultAllowed,
  type PermissionName,
  type SessionKindName,
} from './components/permissions';
export {
  pushHostEvent,
  dismissIncomingRequest,
  useIncomingRequest,
  getHostRoleActions,
  setHostRoleActions,
  type IncomingRequest,
  type HostRoleActions,
} from './components/host-role';
export { RequestDialog, type RequestDialogProps } from './components/request-dialog';
export {
  MorphToolbar,
  ToolbarButton,
  ToolbarSeparator,
  TOOLBAR_EDGES,
  type ToolbarEdge,
} from './components/morph-toolbar';
export {
  loadHostExtras,
  type HostExtras,
  type StreamQuality,
  type TrustedPeer,
  type UpdateCheck,
} from './components/host-extras';
