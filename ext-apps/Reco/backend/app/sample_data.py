"""Sample datasets reproducing the demo reconciliation exactly.

Records are keyed by their on-disk headers so they exercise the same
auto-mapping path as real uploads.
"""

from __future__ import annotations

import csv
import io

BOOKS_CSV = """Supplier GSTIN,Supplier Name,Invoice Number,Invoice Date,Taxable Amount,IGST,CGST,SGST
27AABCU9603R1ZM,Tata Steel Ltd,INV-2024-001,15-12-2025,500000,45000,0,0
29AADCB2230M1ZT,Infosys Ltd,INF/23-24/0456,15-12-2025,200000,0,18000,18000
33AABCT1332L1ZZ,TCS,TCS/2024/100,15-12-2025,80000,0,7200,7200
24AABCW8764Q1ZE,Wipro Ltd,WIP/2024/118,15-12-2025,120000,0,10800,10800
06AABCH1234P1ZQ,HCL Tech,HCL/2024/077,15-12-2025,30000,0,2700,2700
07AAACR5055K1Z0,Reliance Industries,RI-7890,15-12-2025,150000,13500,0,0
18AABCU5674R1ZA,Bajaj Auto Ltd,BAJ/2024/2456,16-12-2025,350000,31500,0,0
08AABCL1234D1ZX,Microsoft India,MS/2024/891,16-12-2025,180000,0,16200,16200
32AABCP9876K1ZY,Oracle Corporation,ORL/2024/234,17-12-2025,420000,0,37800,37800
10AABCR3456M1ZW,Amazon Business,AMZ/2024/567,17-12-2025,95000,0,8550,8550
"""

GSTR2B_CSV = """Supplier GSTIN,Supplier Name,Invoice No,Invoice Date,Taxable,IGST,CGST,SGST
27AABCU9603R1ZM,Tata Steel Ltd,INV-2024-001,15-12-2025,500000,45000,0,0
29AADCB2230M1ZT,Infosys Ltd,INF/23-24/0456,15-12-2025,200000,0,18000,18000
33AABCT1332L1ZZ,TCS,TCS/2024/100,15-12-2025,80000,0,7200,7200
07AAACR5055K1Z0,Reliance Industries,RI-7890,15-12-2025,152000,13680,0,0
05AAACF1234F1ZP,Flipkart Internet Pvt Ltd,FLIP/2024/012,15-12-2025,45000,0,4050,4050
18AABCU5674R1ZA,Bajaj Auto Ltd,BAJ/2024/2456,16-12-2025,350000,31500,0,0
08AABCL1234D1ZX,Microsoft India,MS/2024/891,16-12-2025,180000,0,16200,16200
32AABCP9876K1ZY,Oracle Corporation,ORL/2024/234,17-12-2025,420000,0,37800,37800
"""


def _parse(csv_text: str) -> list[dict]:
    reader = csv.DictReader(io.StringIO(csv_text.strip()))
    return [
        {key.strip(): (value.strip() if isinstance(value, str) else value) for key, value in row.items()}
        for row in reader
    ]


def books_rows() -> list[dict]:
    return _parse(BOOKS_CSV)


def gstr2b_rows() -> list[dict]:
    return _parse(GSTR2B_CSV)
