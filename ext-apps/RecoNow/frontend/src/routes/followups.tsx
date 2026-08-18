import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function FollowupsRoute() {
  return <GenericScreen config={screenConfig("followups")} />;
}
