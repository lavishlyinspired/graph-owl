import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function GstinsRoute() {
  return <GenericScreen config={screenConfig("gstins")} />;
}
