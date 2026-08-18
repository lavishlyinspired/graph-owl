import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function ImsRoute() {
  return <GenericScreen config={screenConfig("ims")} />;
}
