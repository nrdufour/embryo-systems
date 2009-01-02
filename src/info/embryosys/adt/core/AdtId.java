/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.core;

import java.util.UUID;

/**
 * @author nrdufour
 * 
 */
public final class AdtId {

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
}
