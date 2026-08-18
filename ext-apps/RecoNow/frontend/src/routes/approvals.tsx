import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function ApprovalsRoute() {
  return <GenericScreen config={screenConfig("approvals")} />;
}
