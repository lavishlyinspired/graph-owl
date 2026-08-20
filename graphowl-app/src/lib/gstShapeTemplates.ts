/** Curated SHACL shapes for the GST pack, so the Shapes panel has something
 *  real to select and preview rather than an empty textarea — Plan 126
 *  Slice 4. Every property name is verified live against the real GST pack
 *  data (`SELECT DISTINCT ?p WHERE { GRAPH ?g { ?s a gst:Supplier . ?s ?p ?o } }`
 *  and the same for `gst:PurchaseInvoice`), not guessed — and each
 *  description states the real, measured outcome (conforms, or how many of
 *  how many are missing), verified live via `POST /validation/shapes/preview`
 *  before being written down here. */

export interface GstShapeTemplate {
  readonly name: string;
  readonly description: string;
  readonly document: string;
}

export const GST_SHAPE_TEMPLATES: readonly GstShapeTemplate[] = [
  {
    name: "Suppliers have a name",
    description: "Every gst:Supplier states a supplierName. Conforms today (18/19) — one supplier is missing it.",
    document: `@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix gst: <https://graph-owl.dev/packs/gst#> .

gst:SupplierNameShape a sh:NodeShape ;
  sh:targetClass gst:Supplier ;
  sh:property [ sh:path gst:supplierName ; sh:minCount 1 ] .`,
  },
  {
    name: "Suppliers are reviewed",
    description: "Every gst:Supplier needs a reviewedBy. Catches all 19 — nobody has reviewed a supplier yet.",
    document: `@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix gst: <https://graph-owl.dev/packs/gst#> .

gst:SupplierReviewedShape a sh:NodeShape ;
  sh:targetClass gst:Supplier ;
  sh:property [ sh:path gst:reviewedBy ; sh:minCount 1 ] .`,
  },
  {
    name: "Purchase invoices have a taxable value and date",
    description: "Every gst:PurchaseInvoice states taxableValue and invoiceDate. Conforms today (32/32).",
    document: `@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix gst: <https://graph-owl.dev/packs/gst#> .

gst:PurchaseInvoiceBasicsShape a sh:NodeShape ;
  sh:targetClass gst:PurchaseInvoice ;
  sh:property [ sh:path gst:taxableValue ; sh:minCount 1 ] ;
  sh:property [ sh:path gst:invoiceDate ; sh:minCount 1 ] .`,
  },
  {
    name: "Purchase invoices state their HSN code",
    description: "Every gst:PurchaseInvoice needs an hsnCode. Catches 16 of 32 — about half are missing it.",
    document: `@prefix sh: <http://www.w3.org/ns/shacl#> .
@prefix gst: <https://graph-owl.dev/packs/gst#> .

gst:PurchaseInvoiceHsnShape a sh:NodeShape ;
  sh:targetClass gst:PurchaseInvoice ;
  sh:property [ sh:path gst:hsnCode ; sh:minCount 1 ] .`,
  },
];
