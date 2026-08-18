import * as React from "react";
import { cn } from "../../lib/utils";

const LOADING_LABEL = "Loading…";

export const Table = React.forwardRef<HTMLTableElement, React.ComponentProps<"table">>(
  ({ className, children, ...props }, ref) => (
    <div className="w-full overflow-auto">
      <table
        ref={ref}
        className={cn(
          "w-full caption-bottom text-sm text-[var(--gowl-text)]",
          className,
        )}
        {...props}
      >
        {children}
      </table>
    </div>
  ),
);
Table.displayName = "Table";

export const TableHeader = React.forwardRef<HTMLTableSectionElement, React.ComponentProps<"thead">>(
  ({ className, children, ...props }, ref) => (
    <thead
      ref={ref}
      className={cn(
        "border-b border-[var(--gowl-border)] bg-[var(--gowl-fill-subtle)]",
        className,
      )}
      {...props}
    >
      {children}
    </thead>
  ),
);
TableHeader.displayName = "TableHeader";

export const TableBody = React.forwardRef<HTMLTableSectionElement, React.ComponentProps<"tbody">>(
  ({ className, children, ...props }, ref) => (
    <tbody ref={ref} className={cn("", className)} {...props}>
      {children}
    </tbody>
  ),
);
TableBody.displayName = "TableBody";

export const TableRow = React.forwardRef<HTMLTableRowElement, React.ComponentProps<"tr">>(
  ({ className, children, ...props }, ref) => (
    <tr
      ref={ref}
      className={cn(
        "border-b border-[var(--gowl-border-soft)] transition-colors hover:bg-[var(--gowl-row-hover)] data-[state=selected]:bg-[var(--gowl-fill)]",
        className,
      )}
      {...props}
    >
      {children}
    </tr>
  ),
);
TableRow.displayName = "TableRow";

export const TableHead = React.forwardRef<HTMLTableCellElement, React.ComponentProps<"th">>(
  ({ className, children, ...props }, ref) => (
    <th
      ref={ref}
      className={cn(
        "h-10 px-4 text-left align-middle font-medium text-[var(--gowl-text-muted)]",
        className,
      )}
      {...props}
    >
      {children}
    </th>
  ),
);
TableHead.displayName = "TableHead";

export const TableCell = React.forwardRef<HTMLTableCellElement, React.ComponentProps<"td">>(
  ({ className, children, ...props }, ref) => (
    <td
      ref={ref}
      className={cn("px-4 py-3 align-middle", className)}
      {...props}
    >
      {children}
    </td>
  ),
);
TableCell.displayName = "TableCell";

export const TableCaption = React.forwardRef<HTMLTableCaptionElement, React.ComponentProps<"caption">>(
  ({ className, children, ...props }, ref) => (
    <caption
      ref={ref}
      className={cn("mt-4 text-xs text-[var(--gowl-text-subtle)]", className)}
      {...props}
    >
      {children}
    </caption>
  ),
);
TableCaption.displayName = "TableCaption";

/** Ant Design `Table` compatibility wrapper: accept columns + dataSource and
 *  render a shadcn-styled table. Does not implement every antd prop; covers the
 *  ones used in this console. */
interface AntTableColumn<T> {
  readonly title: React.ReactNode;
  readonly dataIndex?: string;
  readonly key?: string;
    // `any`, not `unknown`: matches antd's own `ColumnType.render` signature.
    // `dataIndex` is a runtime-keyed lookup with no static type, and each
    // caller's `render` narrows to its own concrete value type (string,
    // number, a union) — `unknown` breaks that contravariantly for every
    // caller in this codebase; `any` is what makes it assignable.
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    readonly render?: (value: any, record: T, index: number) => React.ReactNode;
  readonly sorter?: (a: T, b: T) => number;
  readonly onCell?: (record: T, index: number) => React.ComponentProps<"td">;
  readonly align?: "left" | "center" | "right";
  readonly width?: number | string;
}

interface AntTableProps<T> {
  readonly dataSource: readonly T[];
  readonly columns: readonly AntTableColumn<T>[];
  readonly rowKey?: string | ((record: T, index: number) => string);
  readonly loading?: boolean;
  readonly pagination?: false | { pageSize?: number; current?: number; total?: number; size?: "small" | "default" };
  readonly size?: "small" | "middle" | "large";
  readonly bordered?: boolean;
  readonly rowSelection?: {
    readonly selectedRowKeys?: string[];
    readonly onChange?: (keys: string[]) => void;
  };
  readonly onRow?: (record: T, index: number) => React.ComponentProps<"tr"> & {
    readonly onClick?: () => void;
    readonly onMouseEnter?: () => void;
    readonly style?: React.CSSProperties;
  };
  readonly rowClassName?: (record: T, index: number) => string;
  readonly locale?: { emptyText?: React.ReactNode };
  readonly className?: string;
  readonly style?: React.CSSProperties;
  readonly childrenColumnName?: string;
  readonly scroll?: { readonly x?: number | string; readonly y?: number | string };
  readonly expandable?: {
    readonly expandedRowRender: (record: T, index: number) => React.ReactNode;
    readonly rowExpandable?: (record: T) => boolean;
  };
  readonly summary?: (data: readonly T[]) => React.ReactNode;
}

function AntTableInner<T>({
  dataSource,
  columns,
  rowKey = "id",
  loading,
  pagination,
  size = "middle",
  bordered,
  rowSelection,
  onRow,
  rowClassName,
  locale,
  className,
  style,
  scroll,
  expandable,
  summary,
}: AntTableProps<T>) {
  const [expandedKeys, setExpandedKeys] = React.useState<Set<string>>(new Set());
  const [sortKey, setSortKey] = React.useState<string | null>(null);
  const [sortDir, setSortDir] = React.useState<"asc" | "desc">("asc");

  const pageSize = pagination ? pagination.pageSize ?? 10 : dataSource.length;
  const current = pagination ? pagination.current ?? 1 : 1;

  const sorted = React.useMemo(() => {
    if (!sortKey) return dataSource;
    const col = columns.find((c) => c.key === sortKey || c.dataIndex === sortKey);
    if (!col?.sorter) return dataSource;
    const next = [...dataSource].sort(col.sorter);
    return sortDir === "asc" ? next : next.reverse();
  }, [dataSource, columns, sortKey, sortDir]);

  const paged = React.useMemo(() => {
    if (pagination === false) return sorted;
    const start = (current - 1) * pageSize;
    return sorted.slice(start, start + pageSize);
  }, [sorted, pagination, current, pageSize]);

  const keyFor = (row: T, index = 0) =>
    typeof rowKey === "function"
      ? rowKey(row, index)
      : String((row as Record<string, unknown>)[rowKey] ?? "");

  const sizeClasses = {
    small: "text-xs",
    middle: "text-sm",
    large: "text-base",
  };

  if (loading) {
    return (
      <div className="flex h-32 items-center justify-center text-[var(--gowl-text-muted)]">
        {LOADING_LABEL}
      </div>
    );
  }

  const scrollStyle: React.CSSProperties = {
    overflowX: scroll?.x ? "auto" : undefined,
    overflowY: scroll?.y ? "auto" : undefined,
    maxHeight: scroll?.y,
  };

  return (
    <div className={cn("w-full", className)} style={style}>
      <div
        className={cn(
          "overflow-hidden rounded-[var(--gowl-radius-card)]",
          bordered && "border border-[var(--gowl-border)]",
        )}
        style={scrollStyle}
      >
        <Table className={sizeClasses[size]}>
          <TableHeader>
            <TableRow>
              {expandable ? <TableHead className="w-8" /> : null}
              {rowSelection ? (
                <TableHead className="w-10">
                  <input
                    type="checkbox"
                    checked={
                      rowSelection.selectedRowKeys?.length === dataSource.length &&
                      dataSource.length > 0
                    }
                    onChange={(e) => {
                      rowSelection.onChange?.(
                        e.target.checked ? dataSource.map(keyFor) : [],
                      );
                    }}
                    aria-label="Select all rows"
                  />
                </TableHead>
              ) : null}
              {columns.map((col) => (
                <TableHead
                  key={col.key ?? col.dataIndex ?? String(col.title)}
                  className={cn(col.align === "center" && "text-center", col.align === "right" && "text-right")}
                >
                  <button
                    type="button"
                    className={cn(
                      "flex items-center gap-1",
                      col.align === "center" && "mx-auto",
                      col.align === "right" && "ml-auto",
                    )}
                    onClick={() => {
                      if (!col.sorter) return;
                      const key = col.key ?? col.dataIndex ?? "";
                      if (sortKey === key) {
                        setSortDir((d) => (d === "asc" ? "desc" : "asc"));
                      } else {
                        setSortKey(key);
                        setSortDir("asc");
                      }
                    }}
                  >
                    {col.title}
                    {col.sorter ? (
                      <span className="text-[var(--gowl-text-subtle)]">
                        {sortKey === (col.key ?? col.dataIndex) ? (sortDir === "asc" ? "↑" : "↓") : "↕"}
                      </span>
                    ) : null}
                  </button>
                </TableHead>
              ))}
            </TableRow>
          </TableHeader>
          <TableBody>
            {paged.length === 0 ? (
              <TableRow>
                <TableCell
                  colSpan={columns.length + (rowSelection ? 1 : 0) + (expandable ? 1 : 0)}
                  className="h-24 text-center text-[var(--gowl-text-muted)]"
                >
                  {locale?.emptyText ?? "No data"}
                </TableCell>
              </TableRow>
            ) : (
              paged.map((row, index) => {
                const rowProps = onRow?.(row, index) ?? {};
                const { onClick, onMouseEnter, style, className: rowClass, ...restRowProps } = rowProps;
                const key = keyFor(row, index);
                const canExpand = expandable && (expandable.rowExpandable?.(row) ?? true);
                const isExpanded = canExpand && expandedKeys.has(key);
                return (
                <React.Fragment key={key}>
                <TableRow
                  className={cn(rowClassName?.(row, index), rowClass)}
                  style={style}
                  onClick={onClick}
                  onMouseEnter={onMouseEnter}
                  {...restRowProps}
                >
                  {expandable ? (
                    <TableCell>
                      {canExpand ? (
                        <button
                          type="button"
                          aria-label={isExpanded ? "Collapse row" : "Expand row"}
                          className="inline-flex h-4 w-4 items-center justify-center rounded text-[var(--gowl-text-subtle)] hover:text-[var(--gowl-text)]"
                          onClick={() =>
                            setExpandedKeys((prev) => {
                              const next = new Set(prev);
                              if (next.has(key)) next.delete(key);
                              else next.add(key);
                              return next;
                            })
                          }
                        >
                          {isExpanded ? "▾" : "▸"}
                        </button>
                      ) : null}
                    </TableCell>
                  ) : null}
                  {rowSelection ? (
                    <TableCell>
                      <input
                        type="checkbox"
                        checked={rowSelection.selectedRowKeys?.includes(key) ?? false}
                        onChange={() => {
                          const keys = new Set(rowSelection.selectedRowKeys ?? []);
                          if (keys.has(key)) keys.delete(key);
                          else keys.add(key);
                          rowSelection.onChange?.(Array.from(keys));
                        }}
                        aria-label={`Select row ${index + 1}`}
                      />
                    </TableCell>
                  ) : null}
                  {columns.map((col) => {
                    const value = col.dataIndex
                      ? (row as Record<string, unknown>)[col.dataIndex]
                      : undefined;
                    return (
                      <TableCell
                        key={col.key ?? col.dataIndex ?? String(col.title)}
                        className={cn(col.align === "center" && "text-center", col.align === "right" && "text-right")}
                        style={col.width !== undefined ? { width: col.width, minWidth: col.width } : undefined}
                        {...(col.onCell?.(row, index) ?? {})}
                      >
                        {col.render
                          ? col.render(value, row, index)
                          : (value as React.ReactNode)}
                      </TableCell>
                    );
                  })}
                </TableRow>
                {isExpanded ? (
                  <TableRow>
                    <TableCell
                      colSpan={columns.length + (rowSelection ? 1 : 0) + 1}
                      className="bg-[var(--gowl-fill-subtle)]"
                    >
                      {expandable?.expandedRowRender(row, index)}
                    </TableCell>
                  </TableRow>
                ) : null}
                </React.Fragment>
              );
            })
            )}
          </TableBody>
          {summary ? (
            <tfoot className="border-t-2 border-[var(--gowl-border)] bg-[var(--gowl-fill-subtle)] font-medium">
              {summary(paged)}
            </tfoot>
          ) : null}
        </Table>
      </div>
    </div>
  );
}

function Summary({ children }: { children: React.ReactNode }) {
  return <tfoot className="border-t-2 border-[var(--gowl-border)] bg-[var(--gowl-fill-subtle)] font-medium">{children}</tfoot>;
}

function SummaryRow({ children, ...props }: React.ComponentProps<"tr">) {
  return <tr {...props}>{children}</tr>;
}

interface SummaryCellProps extends Omit<React.ComponentProps<"td">, "align"> {
  readonly index?: number;
  readonly align?: "left" | "center" | "right";
}

function SummaryCell({ children, className, index, align, ...props }: SummaryCellProps) {
  void index;
  return (
    <td
      className={cn(
        "px-4 py-3 align-middle",
        align === "center" && "text-center",
        align === "right" && "text-right",
        className,
      )}
      {...props}
    >
      {children}
    </td>
  );
}

const SummaryWithParts = Object.assign(Summary, { Row: SummaryRow, Cell: SummaryCell }) as typeof Summary & {
  Row: typeof SummaryRow;
  Cell: typeof SummaryCell;
};

export const AntTable = Object.assign(AntTableInner, { Summary: SummaryWithParts }) as typeof AntTableInner & {
  Summary: typeof SummaryWithParts;
};
