/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 * 
 * This source code is released under the BSD License.
 */
package info.embryosys.adt.core;

import java.util.UUID;

/**
 * @author nrdufour
 * 
 */
public final class AdtId implements Comparable<AdtId> {

	private final AdtType type;
	private UUID id;

	/**
	 * 
	 */
	public AdtId(final AdtType type, final UUID id) {
		if (type == null || id == null) {
			throw new IllegalArgumentException();
		}

		this.type = type;
		this.id = id;
	}

	/**
	 * @return the id
	 */
	public UUID getId() {
		return id;
	}

	/**
	 * @return the type
	 */
	public AdtType getType() {
		return type;
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see java.lang.Object#equals(java.lang.Object)
	 */
	@Override
	public boolean equals(Object obj) {
		if (obj == null)
			return false;
		if (obj == this)
			return true;
		if (!obj.getClass().equals(this.getClass()))
			return false;

		AdtId other = (AdtId) obj;

		return this.type.equals(other.type) && this.id.equals(other.id);
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see java.lang.Object#hashCode()
	 */
	@Override
	public int hashCode() {
		return this.id.hashCode() + 31 * this.type.hashCode();
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see java.lang.Object#toString()
	 */
	@Override
	public String toString() {
		StringBuffer buffer = new StringBuffer();

		buffer.append("AdtId(type: ");
		buffer.append(this.type);
		buffer.append(", id: ");
		buffer.append(this.id);
		buffer.append(")");

		return buffer.toString();
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see java.lang.Comparable#compareTo(java.lang.Object)
	 */
	@Override
	public int compareTo(AdtId o) {
		if (o == this)
			return 0;

		return this.toString().compareTo(o.toString());
	}
}
