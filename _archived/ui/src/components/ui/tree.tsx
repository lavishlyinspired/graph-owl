import * as React from "react";
import { cn } from "../../lib/utils";
import { CaretDownOutlined, CaretRightOutlined } from "./icons";

export interface TreeDataNode {
  readonly key: string;
  readonly title: React.ReactNode;
  readonly children?: TreeDataNode[];
  readonly disabled?: boolean;
  readonly icon?: React.ReactNode;
  readonly selectable?: boolean;
  readonly isLeaf?: boolean;
}

interface TreeProps extends Omit<React.ComponentProps<"div">, "onSelect" | "onClick"> {
  readonly treeData: TreeDataNode[];
  readonly selectedKeys?: string[];
  readonly expandedKeys?: string[];
  readonly defaultExpandedKeys?: string[];
  readonly onSelect?: (keys: string[], info: { node: { key: string } }) => void;
  readonly onExpand?: (keys: string[], info: { expanded: boolean; node: { key: string } }) => void;
  readonly onClick?: (info: { key: string }) => void;
  readonly showLine?: boolean;
  readonly showIcon?: boolean;
  readonly blockNode?: boolean;
  readonly multiple?: boolean;
  readonly loadData?: (node: TreeDataNode) => Promise<void>;
}

function TreeNodeItem({
  node,
  depth,
  selectedKeys,
  expandedKeys,
  showIcon,
  blockNode,
  multiple,
  loadData,
  onSelect,
  onExpand,
  onClick,
}: {
  node: TreeDataNode;
  depth: number;
  selectedKeys: string[];
  expandedKeys: string[];
  showIcon?: boolean;
  blockNode?: boolean;
  multiple?: boolean;
  loadData?: (node: TreeDataNode) => Promise<void>;
  onSelect?: (keys: string[], info: { node: { key: string } }) => void;
  onExpand?: (keys: string[], info: { expanded: boolean; node: { key: string } }) => void;
  onClick?: (info: { key: string }) => void;
}) {
  const hasChildren = (node.children?.length ?? 0) > 0;
  const isLeaf = node.isLeaf ?? !hasChildren;
  const expanded = expandedKeys.includes(node.key);
  const selected = selectedKeys.includes(node.key);
  const [loading, setLoading] = React.useState(false);

  const toggle = async (e?: React.MouseEvent) => {
    e?.stopPropagation();
    const next = new Set(expandedKeys);
    const willExpand = !next.has(node.key);
    if (willExpand) next.add(node.key);
    else next.delete(node.key);
    onExpand?.(Array.from(next), { expanded: willExpand, node: { key: node.key } });
    if (willExpand && loadData && !hasChildren && !node.isLeaf) {
        setLoading(true);
        try {
          await loadData(node);
        } finally {
          setLoading(false);
        }
      }
  };

  return (
    <div>
      <div
        className={cn(
          "flex cursor-pointer items-center gap-1 rounded-[var(--gowl-radius-control)] px-1 py-1 text-sm transition-colors",
          selected
            ? "bg-[var(--gowl-selected)] font-medium text-[var(--gowl-selected-text)]"
            : "text-[var(--gowl-text)] hover:bg-[var(--gowl-fill)]",
          node.disabled && "pointer-events-none opacity-50",
        )}
        style={{ paddingLeft: `${depth * 16 + 8}px` }}
        onClick={() => {
          if (node.selectable !== false) {
            const nextKeys =
              multiple && selectedKeys.includes(node.key)
                ? selectedKeys.filter((k) => k !== node.key)
                : multiple
                  ? [...selectedKeys, node.key]
                  : [node.key];
            onSelect?.(nextKeys, { node: { key: node.key } });
          }
          onClick?.({ key: node.key });
          if (blockNode) toggle();
        }}
      >
        {!isLeaf ? (
          <button
            type="button"
            className="inline-flex h-4 w-4 items-center justify-center rounded text-[var(--gowl-text-subtle)] hover:text-[var(--gowl-text)]"
            onClick={(e) => toggle(e)}
          >
            {loading ? (
              <span className="h-3 w-3 animate-spin rounded-full border border-[var(--gowl-border)] border-t-[var(--gowl-primary)]" />
            ) : expanded ? (
              <CaretDownOutlined size={12} />
            ) : (
              <CaretRightOutlined size={12} />
            )}
          </button>
        ) : (
          <span className="h-4 w-4" />
        )}
        {showIcon && node.icon ? (
          <span className="mr-1 inline-flex text-[var(--gowl-text-subtle)]">{node.icon}</span>
        ) : null}
        <span className="truncate">{node.title}</span>
      </div>
      {expanded && (hasChildren || (!isLeaf && !loadData)) ? (
        <div>
          {node.children?.map((child) => (
            <TreeNodeItem
              key={child.key}
              node={child}
              depth={depth + 1}
              selectedKeys={selectedKeys}
              expandedKeys={expandedKeys}
              showIcon={showIcon}
              blockNode={blockNode}
              multiple={multiple}
              loadData={loadData}
              onSelect={onSelect}
              onExpand={onExpand}
              onClick={onClick}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

export const Tree = React.forwardRef<HTMLDivElement, TreeProps>(
  (
    {
      className,
      treeData,
      selectedKeys,
      expandedKeys: controlledExpanded,
      defaultExpandedKeys,
      onSelect,
      onExpand,
      onClick,
      showIcon,
      showLine,
      blockNode,
      multiple,
      loadData,
      ...props
    },
    ref,
  ) => {
    void showLine;
    const isExpandedControlled = controlledExpanded !== undefined;
    const [internalExpanded, setInternalExpanded] = React.useState<string[]>(
      defaultExpandedKeys ?? [],
    );
    const expanded = isExpandedControlled ? controlledExpanded : internalExpanded;

    return (
      <div ref={ref} className={cn("flex flex-col text-sm", className)} {...props}>
        {treeData.map((node) => (
          <TreeNodeItem
            key={node.key}
            node={node}
            depth={0}
            selectedKeys={selectedKeys ?? []}
            expandedKeys={expanded}
            showIcon={showIcon}
            blockNode={blockNode}
            multiple={multiple}
            loadData={loadData}
            onSelect={onSelect}
            onExpand={(keys, info) => {
              if (!isExpandedControlled) setInternalExpanded(keys);
              onExpand?.(keys, info);
            }}
            onClick={onClick}
          />
        ))}
      </div>
    );
  },
);
Tree.displayName = "Tree";
