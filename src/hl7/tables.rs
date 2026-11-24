pub(super) const JP_JIS0208DECODE: super::charset::Table = super::charset::Table {
    offset: 0,
    len: 11104,
    width: 2,
};
pub(super) const JP_JIS0212DECODE: super::charset::Table = super::charset::Table {
    offset: 22208,
    len: 7211,
    width: 2,
};
pub(super) const JP_ENCODE0: super::charset::Table = super::charset::Table {
    offset: 36630,
    len: 20902,
    width: 2,
};
pub(super) const JP_ENCODE1: super::charset::Table = super::charset::Table {
    offset: 78434,
    len: 1632,
    width: 2,
};
pub(super) const JP_ENCODE2: super::charset::Table = super::charset::Table {
    offset: 81698,
    len: 974,
    width: 2,
};
pub(super) const JP_ENCODE3: super::charset::Table = super::charset::Table {
    offset: 83646,
    len: 959,
    width: 2,
};
pub(super) const JP_ENCODE4: super::charset::Table = super::charset::Table {
    offset: 85564,
    len: 261,
    width: 2,
};
pub(super) const JP_ENCODE5: super::charset::Table = super::charset::Table {
    offset: 86086,
    len: 229,
    width: 2,
};
pub(super) const JP_JIS0208: u32 = 1;
pub(super) const JP_JIS0212: u32 = 2;
pub(super) const JP_CODEMASK: u32 = 127;
pub(super) const JP_CODESHIFT: u32 = 7;
pub(super) const JP_TABLESHIFT: u32 = 14;
pub(super) const JP_ENCODE0LOW: u32 = 19968;
pub(super) const JP_ENCODE0HIGH: u32 = 40870;
pub(super) const JP_ENCODE1LOW: u32 = 8208;
pub(super) const JP_ENCODE1HIGH: u32 = 9840;
pub(super) const JP_ENCODE2LOW: u32 = 12288;
pub(super) const JP_ENCODE2HIGH: u32 = 13262;
pub(super) const JP_ENCODE3LOW: u32 = 161;
pub(super) const JP_ENCODE3HIGH: u32 = 1120;
pub(super) const JP_ENCODE4LOW: u32 = 63785;
pub(super) const JP_ENCODE4HIGH: u32 = 64046;
pub(super) const JP_ENCODE5LOW: u32 = 65281;
pub(super) const JP_ENCODE5HIGH: u32 = 65510;
pub(super) const KR_DECODE: super::charset::Table = super::charset::Table {
    offset: 86544,
    len: 17630,
    width: 2,
};
pub(super) const KR_ENCODE0: super::charset::Table = super::charset::Table {
    offset: 121804,
    len: 20893,
    width: 2,
};
pub(super) const KR_ENCODE1: super::charset::Table = super::charset::Table {
    offset: 163590,
    len: 11172,
    width: 2,
};
pub(super) const KR_ENCODE2: super::charset::Table = super::charset::Table {
    offset: 185934,
    len: 1625,
    width: 2,
};
pub(super) const KR_ENCODE3: super::charset::Table = super::charset::Table {
    offset: 189184,
    len: 990,
    width: 2,
};
pub(super) const KR_ENCODE4: super::charset::Table = super::charset::Table {
    offset: 191164,
    len: 945,
    width: 2,
};
pub(super) const KR_ENCODE5: super::charset::Table = super::charset::Table {
    offset: 193054,
    len: 268,
    width: 2,
};
pub(super) const KR_ENCODE6: super::charset::Table = super::charset::Table {
    offset: 193590,
    len: 230,
    width: 2,
};
pub(super) const KR_ENCODE0LOW: u32 = 19968;
pub(super) const KR_ENCODE0HIGH: u32 = 40861;
pub(super) const KR_ENCODE1LOW: u32 = 44032;
pub(super) const KR_ENCODE1HIGH: u32 = 55204;
pub(super) const KR_ENCODE2LOW: u32 = 8213;
pub(super) const KR_ENCODE2HIGH: u32 = 9838;
pub(super) const KR_ENCODE3LOW: u32 = 12288;
pub(super) const KR_ENCODE3HIGH: u32 = 13278;
pub(super) const KR_ENCODE4LOW: u32 = 161;
pub(super) const KR_ENCODE4HIGH: u32 = 1106;
pub(super) const KR_ENCODE5LOW: u32 = 63744;
pub(super) const KR_ENCODE5HIGH: u32 = 64012;
pub(super) const KR_ENCODE6LOW: u32 = 65281;
pub(super) const KR_ENCODE6HIGH: u32 = 65511;
pub(super) const GB_GB18030: super::charset::Table = super::charset::Table {
    offset: 194050,
    len: 412,
    width: 2,
};
pub(super) const GB_DECODE: super::charset::Table = super::charset::Table {
    offset: 194874,
    len: 23845,
    width: 2,
};
pub(super) const GB_ENCODE0: super::charset::Table = super::charset::Table {
    offset: 242564,
    len: 28965,
    width: 2,
};
pub(super) const GB_ENCODE1: super::charset::Table = super::charset::Table {
    offset: 300494,
    len: 1587,
    width: 2,
};
pub(super) const GB_ENCODE2: super::charset::Table = super::charset::Table {
    offset: 303668,
    len: 942,
    width: 2,
};
pub(super) const GB_ENCODE3: super::charset::Table = super::charset::Table {
    offset: 305552,
    len: 438,
    width: 2,
};
pub(super) const GB_ENCODE4: super::charset::Table = super::charset::Table {
    offset: 306428,
    len: 254,
    width: 2,
};
pub(super) const GB_ENCODE0LOW: u32 = 11905;
pub(super) const GB_ENCODE0HIGH: u32 = 40870;
pub(super) const GB_ENCODE1LOW: u32 = 8208;
pub(super) const GB_ENCODE1HIGH: u32 = 9795;
pub(super) const GB_ENCODE2LOW: u32 = 164;
pub(super) const GB_ENCODE2HIGH: u32 = 1106;
pub(super) const GB_ENCODE3LOW: u32 = 65072;
pub(super) const GB_ENCODE3HIGH: u32 = 65510;
pub(super) const GB_ENCODE4LOW: u32 = 63788;
pub(super) const GB_ENCODE4HIGH: u32 = 64042;
pub(super) const BIG5_DECODE: super::charset::Table = super::charset::Table {
    offset: 306936,
    len: 19782,
    width: 4,
};
pub(super) const BIG5_ENCODE0: super::charset::Table = super::charset::Table {
    offset: 386064,
    len: 42633,
    width: 2,
};
pub(super) const BIG5_ENCODE1: super::charset::Table = super::charset::Table {
    offset: 471330,
    len: 29004,
    width: 2,
};
pub(super) const BIG5_ENCODE2: super::charset::Table = super::charset::Table {
    offset: 529338,
    len: 2176,
    width: 2,
};
pub(super) const BIG5_ENCODE3: super::charset::Table = super::charset::Table {
    offset: 533690,
    len: 939,
    width: 2,
};
pub(super) const BIG5_ENCODE4: super::charset::Table = super::charset::Table {
    offset: 535568,
    len: 446,
    width: 2,
};
pub(super) const BIG5_ENCODE5: super::charset::Table = super::charset::Table {
    offset: 536460,
    len: 432,
    width: 2,
};
pub(super) const BIG5_ENCODE6: super::charset::Table = super::charset::Table {
    offset: 537324,
    len: 263,
    width: 2,
};
pub(super) const BIG5_ENCODE7: super::charset::Table = super::charset::Table {
    offset: 537850,
    len: 1,
    width: 2,
};
pub(super) const BIG5_ENCODE0LOW: u32 = 131105;
pub(super) const BIG5_ENCODE0HIGH: u32 = 173738;
pub(super) const BIG5_ENCODE1LOW: u32 = 11904;
pub(super) const BIG5_ENCODE1HIGH: u32 = 40908;
pub(super) const BIG5_ENCODE2LOW: u32 = 7870;
pub(super) const BIG5_ENCODE2HIGH: u32 = 10046;
pub(super) const BIG5_ENCODE3LOW: u32 = 167;
pub(super) const BIG5_ENCODE3HIGH: u32 = 1106;
pub(super) const BIG5_ENCODE4LOW: u32 = 65072;
pub(super) const BIG5_ENCODE4HIGH: u32 = 65518;
pub(super) const BIG5_ENCODE5LOW: u32 = 194597;
pub(super) const BIG5_ENCODE5HIGH: u32 = 195029;
pub(super) const BIG5_ENCODE6LOW: u32 = 63751;
pub(super) const BIG5_ENCODE6HIGH: u32 = 64014;
pub(super) const BIG5_ENCODE7LOW: u32 = 175615;
pub(super) const BIG5_ENCODE7HIGH: u32 = 175616;
pub(super) const ISO8859_1_DECODE: super::charset::Table = super::charset::Table {
    offset: 537852,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_1_ENCODE: super::charset::Table = super::charset::Table {
    offset: 538364,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_2_DECODE: super::charset::Table = super::charset::Table {
    offset: 538876,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_2_ENCODE: super::charset::Table = super::charset::Table {
    offset: 539388,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_3_DECODE: super::charset::Table = super::charset::Table {
    offset: 539900,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_3_ENCODE: super::charset::Table = super::charset::Table {
    offset: 540412,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_4_DECODE: super::charset::Table = super::charset::Table {
    offset: 540924,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_4_ENCODE: super::charset::Table = super::charset::Table {
    offset: 541436,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_5_DECODE: super::charset::Table = super::charset::Table {
    offset: 541948,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_5_ENCODE: super::charset::Table = super::charset::Table {
    offset: 542460,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_6_DECODE: super::charset::Table = super::charset::Table {
    offset: 542972,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_6_ENCODE: super::charset::Table = super::charset::Table {
    offset: 543484,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_7_DECODE: super::charset::Table = super::charset::Table {
    offset: 543996,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_7_ENCODE: super::charset::Table = super::charset::Table {
    offset: 544508,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_8_DECODE: super::charset::Table = super::charset::Table {
    offset: 545020,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_8_ENCODE: super::charset::Table = super::charset::Table {
    offset: 545532,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_9_DECODE: super::charset::Table = super::charset::Table {
    offset: 546044,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_9_ENCODE: super::charset::Table = super::charset::Table {
    offset: 546556,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_15_DECODE: super::charset::Table = super::charset::Table {
    offset: 547068,
    len: 256,
    width: 2,
};
pub(super) const ISO8859_15_ENCODE: super::charset::Table = super::charset::Table {
    offset: 547580,
    len: 256,
    width: 2,
};
