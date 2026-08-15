# RecoNow — Sample Data Files

## Files in this folder

| File | Description | Rows |
|------|-------------|------|
| `purchase_register_mar2026.csv` | Your company's Purchase Register (Books) for March 2026 | 16 |
| `gstr2b_mar2026.csv` | GSTR-2B downloaded from GST Portal for March 2026 | 15 |

---

## How to use

1. Open RecoNow (`./launch.sh` → http://localhost:5173)
2. On the **Upload** page, drag both CSV files together into the drop zone
3. Go through **Map → Reconcile → Intelligence → Act**

---

## What's in the data

The sample data is deliberately crafted to exercise every reconciliation scenario:

| Scenario | Invoice(s) | Expected Result |
|----------|-----------|-----------------|
| **Exact match** | INV-MAR-001 to INV-MAR-010 | ✅ Matched — Exact |
| **Tolerance match** (diff < ₹1) | INV-MAR-012 | ✅ Matched — Within Tolerance |
| **Credit note match** | CN-MAR-001 | ✅ Matched — Exact |
| **Amount discrepancy** (diff ₹500) | INV-MAR-011 | ⚠️ Review — Amount Diff |
| **Only in Books** (supplier non-filer) | INV-MAR-013, INV-MAR-014 | 🔴 Not in GSTR-2B |
| **Only in Portal** (missed in books) | EXTRA-2B-001 | 🔵 Not in Books |
| **Reverse charge (RCM)** | INV-MAR-015 | ✅ Matched — Exact |

### Expected Reconciliation Stats (March 2026)
- **Total rows:** 17
- **Matched:** 13 (~76%)
- **Review / Mismatch:** 1
- **Only in Books (non-filers):** 2
- **Only in Portal (missed):** 1

---

## Column Mapping Reference

### Purchase Register (Books)
| Column | Maps to |
|--------|---------|
| Invoice No | Invoice Number |
| Invoice Date | Invoice Date |
| Supplier GSTIN | Supplier GSTIN |
| Supplier Name | Supplier Name |
| Taxable Amount | Taxable Amount |
| IGST | IGST |
| CGST | CGST |
| SGST | SGST |
| Cess | Cess |
| HSN Code | HSN Code |
| Place of Supply | Place of Supply |
| Voucher No | Voucher No |
| Voucher Type | Voucher Type |
| Reverse Charge | Reverse Charge |
| Note Type | Note Type |
| Original Invoice No | Original Invoice No |
| IMS Status | IMS Status |

### GSTR-2B
| Column | Maps to |
|--------|---------|
| Invoice No | Invoice Number |
| Invoice Date | Invoice Date |
| GSTIN of Supplier | Supplier GSTIN |
| Supplier Name | Supplier Name |
| Taxable Value | Taxable Amount |
| Integrated Tax | IGST |
| Central Tax | CGST |
| State/UT Tax | SGST |
| Cess | Cess |
| HSN/SAC | HSN Code |
| Place of Supply | Place of Supply |
| IMS Status | IMS Status |
| Note Type | Note Type |

---

## Supplier Key
| Supplier | GSTIN | Notes |
|----------|-------|-------|
| Sharma Infrastructure Pvt Ltd | 27AABCS1429B1Z8 | 2 invoices incl. discrepancy |
| TechCorp Solutions LLP | 29AACCS9460D1Z4 | CGST+SGST, tolerance match |
| Allied Services Ltd | 06AAKCA0977G1Z3 | Has credit note |
| Tamil Traders Company | 33AABCT6996D1ZX | CGST+SGST |
| Naresh Enterprises Delhi | 07AAACN0082H1ZJ | IGST |
| Patel Chemicals & Co | 19AABCP8087C1ZV | IGST + Cess |
| Rajan Textiles Exports | 24AABCR6898D1ZY | CGST+SGST |
| Vijay Auto Components | 36AAACV1234P1ZH | IGST |
| Mohan & Sons Kanpur | 09AABCM4567R1ZK | IGST |
| Kerala Logistics Hub | 32AABCK1112B1ZP | CGST+SGST |
| Legal Advisory Services | 24AABCL5566H1ZR | RCM invoice |
| **Ghost Vendor Pvt Ltd** | 11AABCZ9999A1Z1 | ⚠️ Non-filer — only in books |
| **Phantom Supplies Co** | 22AABCX8888B1ZQ | ⚠️ Non-filer — only in books |
| **Ferrous Metals Pondicherry** | 34AABCF5678G1ZM | 🔵 Only in portal |
