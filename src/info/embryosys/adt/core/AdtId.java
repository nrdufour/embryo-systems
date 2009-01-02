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
public class AdtId {

	private final AdtType type;
	private UUID id;

	/**
	 * 
	 */
	public AdtId(final AdtType type, final UUID id) {
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
}
