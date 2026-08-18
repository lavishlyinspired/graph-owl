import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function ResetRoute() {
  return <GenericScreen config={screenConfig("reset")} />;
}
