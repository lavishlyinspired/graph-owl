import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function MappingsRoute() {
  return <GenericScreen config={screenConfig("mappings")} />;
}
